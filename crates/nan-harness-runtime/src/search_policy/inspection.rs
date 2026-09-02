use super::MAX_CONFIGURATION_BYTES;
use super::errors::SearchPolicyError;
use super::signal::DetectionSignal;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn detect(candidates: &[PathBuf]) -> Result<DetectionSignal, SearchPolicyError> {
    let mut detected = DetectionSignal::None;
    for path in candidates {
        let metadata = match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(SearchPolicyError::ReadConfiguration {
                    path: path.clone(),
                    source,
                });
            }
        };
        if metadata.len() > MAX_CONFIGURATION_BYTES {
            return Err(SearchPolicyError::ConfigurationTooLarge(path.clone()));
        }
        let contents =
            fs::read_to_string(path).map_err(|source| SearchPolicyError::ReadConfiguration {
                path: path.clone(),
                source,
            })?;
        let signal = inspect_configuration(path, &contents)?;
        if matches!(signal, DetectionSignal::Collision(_)) {
            return Ok(signal);
        }
        detected = detected.combine(signal);
    }
    Ok(detected)
}

pub(super) fn inspect_configuration(
    path: &Path,
    contents: &str,
) -> Result<DetectionSignal, SearchPolicyError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("toml") => {
            let value: toml::Value =
                toml::from_str(contents).map_err(|source| SearchPolicyError::ParseToml {
                    path: path.to_path_buf(),
                    source,
                })?;
            let value =
                serde_json::to_value(value).map_err(|source| SearchPolicyError::ConvertToml {
                    path: path.to_path_buf(),
                    source,
                })?;
            Ok(inspect_value(&value, path))
        }
        Some("yaml" | "yml") => Ok(inspect_yaml(contents, path)),
        _ => {
            let value: Value = jsonc_parser::parse_to_serde_value(
                contents,
                &jsonc_parser::ParseOptions::default(),
            )
            .map_err(|source| SearchPolicyError::ParseJson {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(inspect_value(&value, path))
        }
    }
}

fn inspect_value(value: &Value, path: &Path) -> DetectionSignal {
    inspect_value_at(value, path, &[])
}

fn inspect_value_at(value: &Value, path: &Path, ancestors: &[String]) -> DetectionSignal {
    if let Value::Array(values) = value {
        return values
            .iter()
            .fold(DetectionSignal::None, |detected, value| {
                detected.combine(inspect_value_at(value, path, ancestors))
            });
    }
    let Value::Object(object) = value else {
        return DetectionSignal::None;
    };
    let mut detected = DetectionSignal::None;
    for (key, value) in object {
        let normalized = normalize(key);
        if matches!(normalized.as_str(), "mcp" | "mcpservers" | "mcpserver")
            && let Value::Object(servers) = value
        {
            detected = detected.combine(inspect_mcp_servers(servers, path));
        }

        let in_search_section = ancestors.iter().any(|ancestor| ancestor.contains("search"))
            || normalized.contains("search");
        let is_selector = matches!(
            normalized.as_str(),
            "searchbackend" | "searchprovider" | "websearchbackend" | "websearchprovider"
        ) || (normalized == "provider" && in_search_section);
        if is_selector && let Some(provider) = value.as_str() {
            detected = detected.combine(provider_signal(provider));
        }
        if normalized == "websearch" && value.as_object().is_some_and(explicitly_enabled) {
            detected = detected.combine(DetectionSignal::External);
        }

        let mut nested = ancestors.to_vec();
        nested.push(normalized);
        detected = detected.combine(inspect_value_at(value, path, &nested));
    }
    detected
}

fn inspect_mcp_servers(servers: &serde_json::Map<String, Value>, path: &Path) -> DetectionSignal {
    let mut detected = DetectionSignal::None;
    for (name, configuration) in servers {
        if !mcp_enabled(configuration) {
            continue;
        }
        let managed = value_contains(configuration, super::MANAGED_MCP_SIGNATURE);
        if name.eq_ignore_ascii_case(super::MCP_SERVER_ID) {
            let signal = if managed {
                DetectionSignal::ManagedNan
            } else {
                DetectionSignal::Collision(path.to_path_buf())
            };
            detected = detected.combine(signal);
            continue;
        }
        if search_like(name) || value_contains_search(configuration) {
            detected = detected.combine(DetectionSignal::External);
        }
    }
    detected
}

fn mcp_enabled(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return true;
    };
    object.get("enabled").and_then(Value::as_bool) != Some(false)
        && object.get("disabled").and_then(Value::as_bool) != Some(true)
}

fn explicitly_enabled(object: &serde_json::Map<String, Value>) -> bool {
    object.get("enabled").and_then(Value::as_bool) == Some(true)
        && object.get("disabled").and_then(Value::as_bool) != Some(true)
}

fn value_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value.contains(needle),
        Value::Array(values) => values.iter().any(|value| value_contains(value, needle)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(needle) || value_contains(value, needle)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn value_contains_search(value: &Value) -> bool {
    match value {
        Value::String(value) => search_like(value),
        Value::Array(values) => values.iter().any(value_contains_search),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| search_like(key) || value_contains_search(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn inspect_yaml(contents: &str, path: &Path) -> DetectionSignal {
    let active = contents
        .lines()
        .map(strip_yaml_comment)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let lower = active.join("\n").to_ascii_lowercase();
    if lower.contains(super::MCP_SERVER_ID) {
        return if lower.contains(super::MANAGED_MCP_SIGNATURE) {
            DetectionSignal::ManagedNan
        } else {
            DetectionSignal::Collision(path.to_path_buf())
        };
    }

    let mut detected = inspect_yaml_components(&active);
    let mut sections = Vec::<(usize, String)>::new();
    for line in &active {
        let indentation = line.len().saturating_sub(line.trim_start().len());
        while sections
            .last()
            .is_some_and(|(section_indent, _)| *section_indent >= indentation)
        {
            sections.pop();
        }
        let trimmed = line.trim();
        let Some((raw_key, raw_value)) = trimmed.trim_start_matches("- ").split_once(':') else {
            continue;
        };
        let key = normalize(raw_key.trim());
        let value = raw_value.trim().trim_matches(['\'', '"']);
        let in_search_section = sections
            .iter()
            .any(|(_, section)| section.contains("search"));
        let in_mcp_section = sections.iter().any(|(_, section)| section.contains("mcp"));
        let is_selector = matches!(
            key.as_str(),
            "searchbackend" | "searchprovider" | "websearchbackend" | "websearchprovider"
        ) || (key == "provider" && in_search_section);
        if is_selector && !value.is_empty() {
            detected = detected.combine(provider_signal(value));
        }
        if in_mcp_section && value.is_empty() && search_like(&key) {
            detected = detected.combine(DetectionSignal::External);
        }
        if value.is_empty() {
            sections.push((indentation, key));
        }
    }
    detected
}

fn inspect_yaml_components(lines: &[&str]) -> DetectionSignal {
    let mut detected = DetectionSignal::None;
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let Some(id) = trimmed.strip_prefix("- id:").map(str::trim) else {
            index += 1;
            continue;
        };
        let start_indent = lines[index]
            .len()
            .saturating_sub(lines[index].trim_start().len());
        let mut end = index + 1;
        while end < lines.len() {
            let next = lines[end];
            let next_indent = next.len().saturating_sub(next.trim_start().len());
            if next_indent <= start_indent && next.trim().starts_with("- id:") {
                break;
            }
            end += 1;
        }
        let disabled = lines[index + 1..end].iter().any(|line| {
            line.trim()
                .split_once(':')
                .is_some_and(|(key, value)| normalize(key) == "disabled" && value.trim() == "true")
        });
        if search_like(id) && !disabled {
            detected = detected.combine(provider_signal(id));
        }
        index = end;
    }
    detected
}

fn strip_yaml_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(content, _)| content)
}

fn provider_signal(provider: &str) -> DetectionSignal {
    let provider = normalize(provider);
    if provider.is_empty() || matches!(provider.as_str(), "none" | "disabled" | "false" | "off") {
        DetectionSignal::None
    } else if matches!(provider.as_str(), "nan" | "nansearch" | "nanharness") {
        DetectionSignal::ManagedNan
    } else {
        DetectionSignal::External
    }
}

fn search_like(value: &str) -> bool {
    normalize(value).contains("search")
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}
