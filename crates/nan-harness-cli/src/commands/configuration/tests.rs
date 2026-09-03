mod documents;
mod lifecycle;
mod paths;
mod plugin_syntax;
mod search_policy;

use super::documents::{
    get_yaml_path, prepare_exact_file, prepare_exact_file_removal, prepare_json,
    prepare_json_removal, prepare_text_block, prepare_yaml, prepare_yaml_removal,
};
use super::*;
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use tempfile::tempdir;

fn test_models() -> Vec<CodingModelProfile> {
    vec![
        CodingModelProfile::generic("qwen3.6"),
        CodingModelProfile::generic("future-model"),
    ]
}

fn test_config() -> ResolvedConfig {
    ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://api.nan.test/v1".to_owned()),
            nan_api_key: Some(
                SecretValue::new("secret-value").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve")
}

fn assert_persistent_search_contract(harness: HarnessKind, home: &Path) {
    let paths = match harness {
        HarnessKind::OpenCode => vec![home.join(".config/opencode/opencode.json")],
        HarnessKind::Hermes => vec![
            home.join(".hermes/config.yaml"),
            home.join(".hermes/plugins/web/nan_harness/provider.py"),
        ],
        HarnessKind::Pi => vec![home.join(".pi/agent/extensions/nan-search.js")],
        HarnessKind::Omp => vec![home.join(".omp/agent/extensions/nan-search.mjs")],
        HarnessKind::PrimeAgent => {
            vec![home.join(".prime/agent/extensions/nan-search.js")]
        }
        HarnessKind::DeepSeekHarness => vec![home.join(".dsh/cordis.patch.yml")],
        HarnessKind::OpenClaw => vec![
            home.join(".openclaw/openclaw.json"),
            home.join(".openclaw/extensions/nan-harness-search/index.js"),
        ],
        HarnessKind::Cline => {
            vec![home.join(".cline/data/settings/mcp_settings.json")]
        }
        HarnessKind::QwenCode => vec![home.join(".qwen/mcp.json")],
        HarnessKind::KimiCode => vec![home.join(".kimi-code/mcp.json")],
        HarnessKind::Goose => vec![home.join(".config/goose/config.yaml")],
        HarnessKind::Aider => {
            let config = fs::read_to_string(home.join(".aider.conf.yml"))
                .expect("Aider configuration should be readable");
            assert!(!config.contains("nan-search"));
            return;
        }
        HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => unreachable!(),
    };
    let combined = paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if matches!(harness, HarnessKind::Pi | HarnessKind::PrimeAgent) {
        assert!(combined.contains("pi.getAllTools()"));
        assert!(combined.contains("getApiKeyForProvider(\"nan\")"));
        assert!(!combined.contains("secret-value"));
    } else if harness == HarnessKind::Omp {
        assert!(combined.contains("ctx.invokeTool"));
        assert!(combined.contains("getApiKey(\"nan\")"));
        assert!(combined.contains("hybridProviders"));
        assert!(!combined.contains("secret-value"));
    } else {
        assert!(
            combined.contains("nan-search"),
            "{harness} did not activate the managed search contract: {paths:?}"
        );
        assert!(
            combined.contains("__search-mcp")
                || matches!(harness, HarnessKind::Hermes | HarnessKind::OpenClaw),
            "{harness} did not use the direct search MCP contract"
        );
    }
}

fn receipt_path(receipt: &DocumentReceipt) -> &Path {
    match receipt {
        DocumentReceipt::Json(receipt) => &receipt.path,
        DocumentReceipt::Yaml(receipt) => &receipt.path,
        DocumentReceipt::TextBlock(receipt) => &receipt.path,
        DocumentReceipt::ExactFile(receipt) => &receipt.path,
        DocumentReceipt::Toml(receipt) => &receipt.path,
    }
}

fn json_receipt(document: &PreparedDocument) -> JsonReceipt {
    match &document.receipt {
        DocumentReceipt::Json(receipt) => receipt.clone(),
        _ => panic!("expected a JSON receipt"),
    }
}
