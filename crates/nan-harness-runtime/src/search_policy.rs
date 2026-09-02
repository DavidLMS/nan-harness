mod candidates;
mod configuration;
mod environment;
mod errors;
mod inspection;
mod policy;
mod signal;

use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
use std::path::Path;

pub use configuration::SearchConfiguration;
pub use errors::SearchPolicyError;
use policy::supports_nan_search;
#[allow(unused_imports)]
pub(crate) use policy::{SearchResolution, resolve};

const MAX_CONFIGURATION_BYTES: u64 = 2 * 1024 * 1024;
const MANAGED_MCP_SIGNATURE: &str = "__search-mcp";
const MCP_SERVER_ID: &str = "nan-search";

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
    candidates::add_harness_candidates(harness, home, working_directory, &mut paths);
    match environment::detect_environment(harness, home)?
        .combine(inspection::detect(&paths.into_iter().collect::<Vec<_>>())?)
    {
        signal::DetectionSignal::None => Ok(SearchConfiguration::None),
        signal::DetectionSignal::ManagedNan => Ok(SearchConfiguration::ManagedNan),
        signal::DetectionSignal::External => Ok(SearchConfiguration::External),
        signal::DetectionSignal::Collision(path) => Err(SearchPolicyError::McpNameCollision(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::candidates::add_harness_candidates;
    use super::environment::{HERMES_SEARCH_ENVIRONMENT, inspect_dotenv};
    use super::inspection::{detect, inspect_configuration};
    use super::policy::resolve_from_candidates;
    use super::signal::DetectionSignal;
    use super::{
        SearchConfiguration, SearchPolicyError, SearchResolution, inspect_search_configuration,
    };
    use nan_harness_core::{HarnessKind, WebSearchPolicy};
    use std::collections::BTreeSet;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn candidates(harness: HarnessKind, home: &Path, working: &Path) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        add_harness_candidates(harness, home, working, &mut paths);
        paths
    }

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
    fn managed_nan_search_is_reused_when_an_external_provider_also_exists() {
        let home = tempfile::tempdir().expect("temporary home");
        let config = home.path().join("config.json");
        fs::write(
            &config,
            r#"{"mcp":{"nan-search":{"command":["nan-harness","__search-mcp"]},"brave-search":{"command":["brave-search"]}}}"#,
        )
        .expect("combined search config should write");

        assert_eq!(
            resolve_from_candidates(WebSearchPolicy::Force, std::slice::from_ref(&config))
                .expect("force should reuse managed NaN search"),
            SearchResolution::Existing
        );
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
                .expect("combined configuration should inspect"),
            SearchConfiguration::ManagedNan
        );

        fs::remove_file(config).expect("managed MCP should be removable");
        assert_eq!(
            inspect_search_configuration(HarnessKind::Cline, &home, &working)
                .expect("external configuration should inspect on its own"),
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

    #[test]
    fn harness_candidate_dispatch_preserves_exact_static_paths() {
        let home = Path::new("/nan-test-home");
        let working = Path::new("/nan-test-working");

        assert_eq!(
            candidates(HarnessKind::ClaudeCode, home, working),
            BTreeSet::from([
                home.join(".claude.json"),
                home.join(".claude/settings.json"),
                working.join(".claude/settings.json"),
            ])
        );
        assert_eq!(
            candidates(HarnessKind::Pi, home, working),
            BTreeSet::from([
                home.join(".pi/agent/settings.json"),
                home.join(".pi/agent/mcp.json"),
                working.join(".pi/settings.json"),
            ])
        );
        assert_eq!(
            candidates(HarnessKind::Cline, home, working),
            BTreeSet::from([
                home.join(".cline/data/settings/mcp_settings.json"),
                home.join(".cline/data/settings/mcp.json"),
                working.join(".cline/mcp.json"),
            ])
        );
        assert_eq!(
            candidates(HarnessKind::OpenClaw, home, working),
            BTreeSet::from([home.join(".openclaw/openclaw.json")])
        );
        assert_eq!(
            candidates(HarnessKind::Aider, home, working),
            BTreeSet::new()
        );
    }

    #[test]
    fn harness_candidate_dispatch_preserves_environment_overrides() {
        let home = Path::new("/nan-test-home");
        let working = Path::new("/nan-test-working");

        let codex_home =
            env::var_os("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::Codex, home, working),
            BTreeSet::from([codex_home.join("config.toml")])
        );

        let config_home =
            env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        let mut opencode = BTreeSet::from([
            config_home.join("opencode/opencode.json"),
            config_home.join("opencode/opencode.jsonc"),
            working.join("opencode.json"),
            working.join("opencode.jsonc"),
        ]);
        if let Some(path) = env::var_os("OPENCODE_CONFIG") {
            opencode.insert(PathBuf::from(path));
        }
        assert_eq!(candidates(HarnessKind::OpenCode, home, working), opencode);

        let hermes_home =
            env::var_os("HERMES_HOME").map_or_else(|| home.join(".hermes"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::Hermes, home, working),
            BTreeSet::from([hermes_home.join("config.yaml")])
        );
        let omp_home = env::var_os("PI_CODING_AGENT_DIR")
            .map_or_else(|| home.join(".omp/agent"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::Omp, home, working),
            BTreeSet::from([
                omp_home.join("config.yml"),
                omp_home.join("config.yaml"),
                working.join(".omp/config.yml"),
            ])
        );
        let prime_home = env::var_os("PRIME_AGENT_CODING_AGENT_DIR")
            .map_or_else(|| home.join(".prime/agent"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::PrimeAgent, home, working),
            BTreeSet::from([
                prime_home.join("settings.json"),
                prime_home.join("mcp.json")
            ])
        );
        let deepseek_home =
            env::var_os("DSH_HOME").map_or_else(|| home.join(".dsh"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::DeepSeekHarness, home, working),
            BTreeSet::from([
                deepseek_home.join("config.yaml"),
                deepseek_home.join("cordis.patch.yml"),
                deepseek_home.join("profiles/default.yaml"),
                deepseek_home.join("profiles/web/cordis.patch.yml"),
            ])
        );
        let qwen_home = env::var_os("QWEN_HOME").map_or_else(|| home.join(".qwen"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::QwenCode, home, working),
            BTreeSet::from([
                qwen_home.join("settings.json"),
                qwen_home.join("mcp.json"),
                working.join(".qwen/settings.json"),
            ])
        );
        let kimi_home =
            env::var_os("KIMI_CODE_HOME").map_or_else(|| home.join(".kimi-code"), PathBuf::from);
        assert_eq!(
            candidates(HarnessKind::KimiCode, home, working),
            BTreeSet::from([kimi_home.join("config.toml"), kimi_home.join("mcp.json")])
        );
        let mut goose = BTreeSet::from([
            config_home.join("goose/config.yaml"),
            config_home.join("goose/profiles.yaml"),
        ]);
        if let Some(additional) = env::var_os("GOOSE_ADDITIONAL_CONFIG_FILES") {
            goose.extend(env::split_paths(&additional));
        }
        assert_eq!(candidates(HarnessKind::Goose, home, working), goose);
        assert_eq!(
            candidates(HarnessKind::Fx, home, working),
            BTreeSet::from([config_home.join("fx/config.json")])
        );
    }
}
