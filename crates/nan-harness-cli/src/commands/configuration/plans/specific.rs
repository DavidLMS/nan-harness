use super::super::{
    CodingModelProfile, ConfigurationError, SEARCH_MCP_ID, Value, YamlValue, dotenv_quote, json,
    yaml_quote,
};
use super::combinators::{exclusive_json, override_json, to_yaml_value};
use super::search::{deepseek_search_plan, search_mcp_plan};
use super::types::{
    DocumentPlan, ExactFilePlan, JsonPlan, TextBlockPlan, YamlEntryMode, YamlEntryPlan, YamlPlan,
};
use super::values::cline_models;
use std::path::Path;

pub(crate) fn cline_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("providers.json"),
            entries: vec![
                exclusive_json(
                    &["providers", "openai-compatible"],
                    json!({
                        "settings": {
                            "apiKey": api_key,
                            "baseUrl": base_url,
                            "model": default_model,
                            "provider": "openai-compatible"
                        },
                        "tokenSource": "manual",
                        "updatedAt": "1970-01-01T00:00:00.000Z"
                    }),
                ),
                override_json(
                    &["lastUsedProvider"],
                    Value::String("openai-compatible".to_owned()),
                ),
                override_json(&["version"], json!(1)),
            ],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("models.json"),
            entries: vec![
                exclusive_json(
                    &["providers", "openai-compatible", "models"],
                    cline_models(models),
                ),
                override_json(&["version"], json!(1)),
            ],
        }),
        search_mcp_plan(
            directory.join("mcp_settings.json"),
            api_key,
            base_url,
            search_managed,
        ),
    ]
}

pub(crate) fn qwen_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join(".env"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_API_KEY={}", dotenv_quote(api_key))),
            conflicting_keys: vec!["NAN_API_KEY=".to_owned()],
        }),
        search_mcp_plan(
            directory.join("mcp.json"),
            api_key,
            base_url,
            search_managed,
        ),
    ]
}

pub(crate) fn deepseek_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    search_managed: bool,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    Ok(vec![
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join(".credentials.yaml"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_API_KEY: {}", yaml_quote(api_key)?)),
            conflicting_keys: vec!["NAN_API_KEY:".to_owned()],
        }),
        deepseek_search_plan(directory, base_url, search_managed)?,
    ])
}

pub(crate) fn goose_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let provider = serde_json::to_vec_pretty(&json!({
        "name": "nan_harness",
        "engine": "openai",
        "display_name": "NaN",
        "description": "NaN models configured by nan-harness",
        "api_key_env": "NAN_HARNESS_API_KEY",
        "base_url": endpoint,
        "models": models.iter().map(|model| json!({
            "name": model.id,
            "context_limit": model.context_window
        })).collect::<Vec<_>>(),
        "supports_streaming": true,
        "requires_auth": true
    }))
    .map_err(ConfigurationError::SerializeDocument)?;
    Ok(vec![
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("custom_providers/nan_harness.json"),
            payload: Some(provider),
        }),
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join("secrets.yaml"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_HARNESS_API_KEY: {}", yaml_quote(api_key)?)),
            conflicting_keys: vec!["NAN_HARNESS_API_KEY:".to_owned()],
        }),
        DocumentPlan::Yaml(YamlPlan {
            path: directory.join("config.yaml"),
            entries: goose_config_entries(api_key, base_url, default_model, search_managed)?,
            legacy_block: Some(super::types::LegacyTextBlock {
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
            }),
        }),
    ])
}

pub(crate) fn goose_config_entries(
    _api_key: &str,
    base_url: &str,
    default_model: &str,
    search_managed: bool,
) -> Result<Vec<YamlEntryPlan>, ConfigurationError> {
    let mut entries = vec![
        YamlEntryPlan {
            path: vec!["GOOSE_PROVIDER".to_owned()],
            value: YamlValue::String("nan_harness".to_owned()),
            mode: YamlEntryMode::Exclusive,
        },
        YamlEntryPlan {
            path: vec!["GOOSE_MODEL".to_owned()],
            value: YamlValue::String(default_model.to_owned()),
            mode: YamlEntryMode::Exclusive,
        },
    ];
    if search_managed {
        entries.push(YamlEntryPlan {
            path: vec!["extensions".to_owned(), SEARCH_MCP_ID.to_owned()],
            value: to_yaml_value(json!({
                "name": SEARCH_MCP_ID,
                "type": "stdio",
                "cmd": "nan-harness",
                "args": [
                    "__search-mcp",
                    "--provider-base-url",
                    base_url,
                    "--token-env",
                    "NAN_HARNESS_API_KEY"
                ],
                "env_keys": ["NAN_HARNESS_API_KEY"],
                "enabled": true,
                "timeout": 60
            }))?,
            mode: YamlEntryMode::Exclusive,
        });
    }
    Ok(entries)
}
