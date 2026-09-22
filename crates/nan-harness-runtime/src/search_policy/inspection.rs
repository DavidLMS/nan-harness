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
        Some("yaml" | "yml") => inspect_yaml(contents, path),
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
    inspect_value_at(value, path, &[], false)
}

fn inspect_value_at(
    value: &Value,
    path: &Path,
    ancestors: &[String],
    yaml_components: bool,
) -> DetectionSignal {
    if let Value::Array(values) = value {
        return values
            .iter()
            .fold(DetectionSignal::None, |detected, value| {
                let component = if yaml_components {
                    value.get("id").and_then(Value::as_str)
                } else {
                    None
                };
                if component.is_some() && !mcp_enabled(value) {
                    return detected;
                }
                let signal = component
                    .filter(|id| search_like(id))
                    .map_or(DetectionSignal::None, provider_signal);
                detected.combine(signal).combine(inspect_value_at(
                    value,
                    path,
                    ancestors,
                    yaml_components,
                ))
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
            if yaml_components {
                // MCP entries own their enabled state; do not rediscover disabled
                // entries or interpret their arguments as native provider selectors.
                continue;
            }
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
        detected = detected.combine(inspect_value_at(value, path, &nested, yaml_components));
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

fn inspect_yaml(contents: &str, path: &Path) -> Result<DetectionSignal, SearchPolicyError> {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(contents).map_err(|source| SearchPolicyError::ParseYaml {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(inspect_value_at(&yaml_search_value(value), path, &[], true))
}

// Keep search-relevant scalar types and string keys. YAML tags (including DeepSeek's
// !!js values) are data here; inspection must never evaluate them.
fn yaml_search_value(value: serde_yaml_ng::Value) -> Value {
    use serde_yaml_ng::Value as Yaml;
    match value {
        Yaml::String(value) => Value::String(value),
        Yaml::Bool(value) => Value::Bool(value),
        Yaml::Sequence(values) => Value::Array(values.into_iter().map(yaml_search_value).collect()),
        Yaml::Mapping(values) => Value::Object(
            values
                .into_iter()
                .filter_map(|(key, value)| {
                    key.as_str()
                        .map(|key| (key.to_owned(), yaml_search_value(value)))
                })
                .collect(),
        ),
        Yaml::Tagged(value) => yaml_search_value(value.value),
        Yaml::Null | Yaml::Number(_) => Value::Null,
    }
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
