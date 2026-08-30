use nan_harness_core::launch_plan::Transport;
use nan_harness_core::{HarnessKind, LaunchPlan, WebSearchPolicy};
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const MAX_CONFIGURATION_BYTES: u64 = 2 * 1024 * 1024;
const MANAGED_MCP_SIGNATURE: &str = "__search-mcp";
const MCP_SERVER_ID: &str = "nan-search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchResolution {
    Nan,
    Existing,
    Disabled,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchConfiguration {
    None,
    ManagedNan,
    External,
    Unsupported,
}

/// Inspects known harness and project configuration without starting a process or making a request.
///
/// # Errors
///
/// Returns an error when a candidate cannot be read or parsed safely, is too large, or owns the
/// reserved `nan-search` MCP identifier without the nan-harness signature.
pub fn inspect_search_configuration(
    harness: HarnessKind,
    home: &Path,
    working_directory: &Path,
) -> Result<SearchConfiguration, SearchPolicyError> {
    if !supports_nan_search(harness) {
        return Ok(SearchConfiguration::Unsupported);
    }
    let mut paths = BTreeSet::new();
    paths.insert(working_directory.join(".mcp.json"));
    add_harness_candidates(harness, home, working_directory, &mut paths);
    match detect_environment(harness, home)?
        .combine(detect(&paths.into_iter().collect::<Vec<_>>())?)
    {
        DetectionSignal::None => Ok(SearchConfiguration::None),
        DetectionSignal::ManagedNan => Ok(SearchConfiguration::ManagedNan),
        DetectionSignal::External => Ok(SearchConfiguration::External),
        DetectionSignal::Collision(path) => Err(SearchPolicyError::McpNameCollision(path)),
    }
}

impl SearchResolution {
    pub(crate) const fn uses_nan(self) -> bool {
        matches!(self, Self::Nan)
    }
}

pub(crate) fn resolve(
    plan: &LaunchPlan,
    direct_chat_gateway: bool,
) -> Result<SearchResolution, SearchPolicyError> {
    if plan.web_search_policy == WebSearchPolicy::Disabled {
        return Ok(SearchResolution::Disabled);
    }
    if !supports_nan_search(plan.harness.kind) {
        return if plan.web_search_policy == WebSearchPolicy::Force {
            Err(SearchPolicyError::UnsupportedHarness(plan.harness.kind))
        } else {
            Ok(SearchResolution::Unsupported)
        };
    }
    let home = home_directory().ok_or(SearchPolicyError::MissingHomeDirectory)?;
    let candidates = candidate_paths(plan, &home);
    let signal = detect_environment(plan.harness.kind, &home)?.combine(detect(&candidates)?);
    if matches!(&plan.transport, Transport::DirectChat { .. }) && !direct_chat_gateway {
        return match (plan.web_search_policy, signal) {
            (_, DetectionSignal::Collision(path)) => Err(SearchPolicyError::McpNameCollision(path)),
            (_, DetectionSignal::ManagedNan)
            | (WebSearchPolicy::Auto, DetectionSignal::External) => Ok(SearchResolution::Existing),
            (WebSearchPolicy::Auto, DetectionSignal::None) => Ok(SearchResolution::Unsupported),
            (WebSearchPolicy::Force, DetectionSignal::External | DetectionSignal::None) => {
                Err(SearchPolicyError::RequiresDirectGateway)
            }
            (WebSearchPolicy::Disabled, _) => unreachable!("disabled returns before detection"),
        };
    }
    resolve_signal(plan.web_search_policy, signal)
}

#[cfg(test)]
fn resolve_from_candidates(
    policy: WebSearchPolicy,
    candidates: &[PathBuf],
) -> Result<SearchResolution, SearchPolicyError> {
    resolve_signal(policy, detect(candidates)?)
}

fn resolve_signal(
    policy: WebSearchPolicy,
    signal: DetectionSignal,
) -> Result<SearchResolution, SearchPolicyError> {
    if let DetectionSignal::Collision(path) = signal {
        return Err(SearchPolicyError::McpNameCollision(path));
    }
    if policy == WebSearchPolicy::Force {
        return Ok(if signal == DetectionSignal::ManagedNan {
            SearchResolution::Existing
        } else {
            SearchResolution::Nan
        });
    }
    Ok(match signal {
        DetectionSignal::External | DetectionSignal::ManagedNan => SearchResolution::Existing,
        DetectionSignal::None => SearchResolution::Nan,
        DetectionSignal::Collision(_) => unreachable!("collision is returned above"),
    })
}

const fn supports_nan_search(harness: HarnessKind) -> bool {
    !matches!(harness, HarnessKind::Aider)
}

fn candidate_paths(plan: &LaunchPlan, home: &Path) -> Vec<PathBuf> {
    let working = Path::new(&plan.process.working_directory);
    let mut paths = BTreeSet::new();
    paths.insert(working.join(".mcp.json"));
    add_harness_candidates(plan.harness.kind, home, working, &mut paths);
    paths.into_iter().collect()
}

fn add_harness_candidates(
    harness: HarnessKind,
    home: &Path,
    working: &Path,
    paths: &mut BTreeSet<PathBuf>,
) {
    match harness {
        HarnessKind::ClaudeCode => {
            paths.extend([
                home.join(".claude.json"),
                home.join(".claude/settings.json"),
                working.join(".claude/settings.json"),
            ]);
        }
        HarnessKind::Codex => {
            paths.insert(
                env::var_os("CODEX_HOME")
                    .map_or_else(|| home.join(".codex"), PathBuf::from)
                    .join("config.toml"),
            );
        }
        HarnessKind::OpenCode => {
            if let Some(path) = env::var_os("OPENCODE_CONFIG") {
                paths.insert(PathBuf::from(path));
            }
            let config_home =
                env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
            paths.extend([
                config_home.join("opencode/opencode.json"),
                config_home.join("opencode/opencode.jsonc"),
                working.join("opencode.json"),
                working.join("opencode.jsonc"),
            ]);
        }
        HarnessKind::Hermes => {
            paths.insert(
                env::var_os("HERMES_HOME")
                    .map_or_else(|| home.join(".hermes"), PathBuf::from)
                    .join("config.yaml"),
            );
        }
        HarnessKind::Pi => {
            paths.extend([
                home.join(".pi/agent/settings.json"),
                home.join(".pi/agent/mcp.json"),
                working.join(".pi/settings.json"),
            ]);
        }
        HarnessKind::PrimeAgent => {
            let prime_home = env::var_os("PRIME_AGENT_CODING_AGENT_DIR")
                .map_or_else(|| home.join(".prime/agent"), PathBuf::from);
            paths.extend([
                prime_home.join("settings.json"),
                prime_home.join("mcp.json"),
            ]);
        }
        HarnessKind::DeepSeekHarness => {
            paths.extend(deepseek_candidate_paths(home));
        }
        HarnessKind::OpenClaw => {
            paths.insert(home.join(".openclaw/openclaw.json"));
        }
        HarnessKind::Cline => {
            paths.extend([
                home.join(".cline/data/settings/mcp_settings.json"),
                home.join(".cline/data/settings/mcp.json"),
                working.join(".cline/mcp.json"),
            ]);
        }
        HarnessKind::QwenCode => {
            let qwen_home =
                env::var_os("QWEN_HOME").map_or_else(|| home.join(".qwen"), PathBuf::from);
            paths.extend([
                qwen_home.join("settings.json"),
                qwen_home.join("mcp.json"),
                working.join(".qwen/settings.json"),
            ]);
        }
        HarnessKind::KimiCode => {
            let kimi_home = env::var_os("KIMI_CODE_HOME")
                .map_or_else(|| home.join(".kimi-code"), PathBuf::from);
            paths.extend([kimi_home.join("config.toml"), kimi_home.join("mcp.json")]);
        }
        HarnessKind::Goose => {
            let config_home =
                env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
            paths.extend([
                config_home.join("goose/config.yaml"),
                config_home.join("goose/profiles.yaml"),
            ]);
            if let Some(additional) = env::var_os("GOOSE_ADDITIONAL_CONFIG_FILES") {
                paths.extend(env::split_paths(&additional));
            }
        }
        HarnessKind::Fx => {
            let config_home =
                env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
            paths.insert(config_home.join("fx/config.json"));
        }
        HarnessKind::Aider => {}
    }
}

fn deepseek_candidate_paths(home: &Path) -> [PathBuf; 4] {
    let deepseek_home = env::var_os("DSH_HOME").map_or_else(|| home.join(".dsh"), PathBuf::from);
    [
        deepseek_home.join("config.yaml"),
        deepseek_home.join("cordis.patch.yml"),
        deepseek_home.join("profiles/default.yaml"),
        deepseek_home.join("profiles/web/cordis.patch.yml"),
    ]
}

const HERMES_SEARCH_ENVIRONMENT: &[&str] = &[
    "BRAVE_SEARCH_API_KEY",
    "EXA_API_KEY",
    "FIRECRAWL_API_KEY",
    "KEENABLE_API_KEY",
    "PARALLEL_API_KEY",
    "SEARXNG_BASE_URL",
    "TAVILY_API_KEY",
];

const OPENCLAW_SEARCH_ENVIRONMENT: &[&str] = &[
    "BRAVE_API_KEY",
    "EXA_API_KEY",
    "FIRECRAWL_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "KIMI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CODE_PLAN_KEY",
    "MINIMAX_CODING_API_KEY",
    "MINIMAX_OAUTH_TOKEN",
    "MOONSHOT_API_KEY",
    "OPENROUTER_API_KEY",
    "PARALLEL_API_KEY",
    "PERPLEXITY_API_KEY",
    "SEARXNG_BASE_URL",
    "TAVILY_API_KEY",
    "XAI_API_KEY",
];

fn detect_environment(
    harness: HarnessKind,
    home: &Path,
) -> Result<DetectionSignal, SearchPolicyError> {
    let (names, dotenv) = match harness {
        HarnessKind::Hermes => {
            let hermes_home =
                env::var_os("HERMES_HOME").map_or_else(|| home.join(".hermes"), PathBuf::from);
            (HERMES_SEARCH_ENVIRONMENT, Some(hermes_home.join(".env")))
        }
        HarnessKind::OpenClaw => (
            OPENCLAW_SEARCH_ENVIRONMENT,
            Some(home.join(".openclaw/.env")),
        ),
        _ => (&[][..], None),
    };
    if names.iter().any(|name| {
        env::var_os(name)
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    }) {
        return Ok(DetectionSignal::External);
    }
    dotenv.map_or(Ok(DetectionSignal::None), |path| {
        inspect_dotenv(&path, names)
    })
}

fn inspect_dotenv(
    path: &Path,
    search_environment: &[&str],
) -> Result<DetectionSignal, SearchPolicyError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(DetectionSignal::None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DetectionSignal::None);
        }
        Err(source) => {
            return Err(SearchPolicyError::ReadConfiguration {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        return Err(SearchPolicyError::ConfigurationTooLarge(path.to_path_buf()));
    }
    let contents =
        fs::read_to_string(path).map_err(|source| SearchPolicyError::ReadConfiguration {
            path: path.to_path_buf(),
            source,
        })?;
    let configured = contents.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            return false;
        };
        search_environment.contains(&name.trim())
            && !value.trim().trim_matches(['\'', '"']).is_empty()
    });
    Ok(if configured {
        DetectionSignal::External
    } else {
        DetectionSignal::None
    })
}

fn detect(candidates: &[PathBuf]) -> Result<DetectionSignal, SearchPolicyError> {
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

fn inspect_configuration(
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
        let managed = value_contains(configuration, MANAGED_MCP_SIGNATURE);
        if name.eq_ignore_ascii_case(MCP_SERVER_ID) {
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
    if lower.contains(MCP_SERVER_ID) {
        return if lower.contains(MANAGED_MCP_SIGNATURE) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum DetectionSignal {
    None,
    ManagedNan,
    External,
    Collision(PathBuf),
}

impl DetectionSignal {
    fn combine(self, other: Self) -> Self {
        match (&self, &other) {
            (Self::Collision(_), _) => self,
            (_, Self::Collision(_)) => other,
            (Self::External, _) => self,
            (_, Self::External) => other,
            (Self::ManagedNan, _) => self,
            (_, Self::ManagedNan) => other,
            (Self::None, Self::None) => Self::None,
        }
    }
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[derive(Debug, Error)]
pub enum SearchPolicyError {
    #[error("NH-SEARCH-POLICY-001")]
    MissingHomeDirectory,
    #[error("NH-SEARCH-POLICY-002: {0}")]
    UnsupportedHarness(HarnessKind),
    #[error("NH-SEARCH-POLICY-003")]
    RequiresDirectGateway,
    #[error("NH-SEARCH-POLICY-004: {0}")]
    McpNameCollision(PathBuf),
    #[error("NH-SEARCH-POLICY-005: {path}")]
    ReadConfiguration {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("NH-SEARCH-POLICY-006: {0}")]
    ConfigurationTooLarge(PathBuf),
    #[error("NH-SEARCH-POLICY-007: {path}")]
    ParseJson {
        path: PathBuf,
        #[source]
        source: jsonc_parser::errors::ParseError,
    },
    #[error("NH-SEARCH-POLICY-008: {path}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("NH-SEARCH-POLICY-009: {path}")]
    ConvertToml {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        DetectionSignal, HERMES_SEARCH_ENVIRONMENT, SearchConfiguration, SearchPolicyError,
        SearchResolution, detect, inspect_configuration, inspect_dotenv,
        inspect_search_configuration, resolve_from_candidates,
    };
    use nan_harness_core::{HarnessKind, WebSearchPolicy};
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn policy_matrix_preserves_external_search_and_force_selects_nan() {
        let home = tempfile::tempdir().expect("temporary home");
        let config = home.path().join("opencode.jsonc");
        fs::write(
            &config,
            r#"{"mcp":{"brave-search":{"type":"local","command":["brave-search"]}}}"#,
        )
        .expect("config should write");

        assert_eq!(
            resolve_from_candidates(WebSearchPolicy::Auto, std::slice::from_ref(&config))
                .expect("auto should resolve"),
            SearchResolution::Existing
        );
        assert_eq!(
            resolve_from_candidates(WebSearchPolicy::Force, std::slice::from_ref(&config))
                .expect("force should resolve"),
            SearchResolution::Nan
        );
        assert_eq!(
            resolve_from_candidates(WebSearchPolicy::Auto, &[]).expect("auto should resolve"),
            SearchResolution::Nan
        );
    }

    #[test]
    fn exact_nan_search_collision_fails_without_starting_the_server() {
        let home = tempfile::tempdir().expect("temporary home");
        let config = home.path().join("config.json");
        fs::write(
            &config,
            r#"{"mcpServers":{"nan-search":{"command":"third-party"}}}"#,
        )
        .expect("config should write");

        assert!(matches!(
            resolve_from_candidates(WebSearchPolicy::Auto, std::slice::from_ref(&config)),
            Err(SearchPolicyError::McpNameCollision(path)) if path == config
        ));
    }

    #[test]
    fn managed_nan_search_is_preserved_and_opaque_mcp_is_ignored() {
        let path = PathBuf::from("config.json");
        let managed = inspect_configuration(
            &path,
            r#"{"mcp":{"nan-search":{"command":["nan-harness","__search-mcp"]}}}"#,
        )
        .expect("managed config should parse");
        assert_eq!(managed, DetectionSignal::ManagedNan);

        let opaque = inspect_configuration(
            &path,
            r#"{"mcp":{"private-tools":{"command":["private-mcp"]}}}"#,
        )
        .expect("opaque config should parse");
        assert_eq!(opaque, DetectionSignal::None);
    }

    #[test]
    fn public_inspection_uses_harness_and_working_directory_candidates() {
        let root = tempfile::tempdir().expect("temporary root");
        let home = root.path().join("home");
        let working = root.path().join("working");
        fs::create_dir_all(home.join(".cline/data/settings")).expect("home should be created");
        fs::create_dir_all(&working).expect("working directory should be created");

        assert_eq!(
            inspect_search_configuration(HarnessKind::Cline, &home, &working)
                .expect("empty configuration should inspect"),
            SearchConfiguration::None
        );

        let config = home.join(".cline/data/settings/mcp_settings.json");
        fs::write(
            &config,
            r#"{"mcpServers":{"nan-search":{"command":"nan-harness","args":["__search-mcp"]}}}"#,
        )
        .expect("managed MCP should write");
        assert_eq!(
            inspect_search_configuration(HarnessKind::Cline, &home, &working)
                .expect("managed configuration should inspect"),
            SearchConfiguration::ManagedNan
        );

        fs::write(
            working.join(".mcp.json"),
            r#"{"webSearch":{"enabled":true}}"#,
        )
        .expect("external configuration should write");
        assert_eq!(
            inspect_search_configuration(HarnessKind::Cline, &home, &working)
                .expect("external configuration should inspect"),
            SearchConfiguration::External
        );
    }

    #[test]
    fn native_provider_selectors_are_detected_in_json_toml_and_yaml() {
        let home = tempfile::tempdir().expect("temporary home");
        let json = home.path().join("config.json");
        let toml = home.path().join("config.toml");
        let yaml = home.path().join("config.yaml");
        let disabled_yaml = home.path().join("disabled.yaml");
        fs::write(&json, r#"{"tools":{"webSearch":{"enabled":true}}}"#).expect("JSON should write");
        fs::write(&toml, "[web]\nsearch_backend = \"tavily\"\n").expect("TOML should write");
        fs::write(&yaml, "web:\n  search_backend: brave\n").expect("YAML should write");
        fs::write(
            &disabled_yaml,
            "- id: web-search-deepseek\n  disabled: true\n",
        )
        .expect("disabled YAML should write");

        for path in [json, toml, yaml] {
            assert_eq!(
                inspect_configuration(&path, &fs::read_to_string(&path).expect("config"))
                    .expect("config should parse"),
                DetectionSignal::External,
                "{}",
                path.display()
            );
        }
        assert_eq!(
            inspect_configuration(
                &disabled_yaml,
                &fs::read_to_string(&disabled_yaml).expect("config")
            )
            .expect("disabled config should parse"),
            DetectionSignal::None
        );
    }

    #[test]
    fn dotenv_detection_checks_only_search_specific_credentials() {
        let home = tempfile::tempdir().expect("temporary home");
        let dotenv = home.path().join(".env");
        fs::write(
            &dotenv,
            "OPENROUTER_API_KEY=model-only\nexport TAVILY_API_KEY='search-key'\n",
        )
        .expect("dotenv should write");

        assert_eq!(
            inspect_dotenv(&dotenv, HERMES_SEARCH_ENVIRONMENT).expect("dotenv detection"),
            DetectionSignal::External
        );
        fs::write(&dotenv, "OPENROUTER_API_KEY=model-only\nTAVILY_API_KEY=\n")
            .expect("dotenv should update");
        assert_eq!(
            inspect_dotenv(&dotenv, HERMES_SEARCH_ENVIRONMENT).expect("dotenv detection"),
            DetectionSignal::None
        );
    }

    #[test]
    fn missing_configuration_detection_stays_below_the_no_mcp_budget() {
        let home = tempfile::tempdir().expect("temporary home");
        let candidates = (0..12)
            .map(|index| home.path().join(format!("missing-{index}.json")))
            .collect::<Vec<_>>();
        let mut timings = (0..101)
            .map(|_| {
                let started = Instant::now();
                assert_eq!(
                    detect(&candidates).expect("detection"),
                    DetectionSignal::None
                );
                started.elapsed()
            })
            .collect::<Vec<_>>();
        timings.sort_unstable();
        assert!(
            timings[timings.len() / 2] < Duration::from_millis(50),
            "median detection was {:?}",
            timings[timings.len() / 2]
        );
    }

    use std::path::PathBuf;
}
