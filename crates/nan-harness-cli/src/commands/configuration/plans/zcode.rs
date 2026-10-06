use super::super::{ConfigurationError, ConfigurationPaths, SEARCH_MCP_ID, json};
use super::combinators::{exclusive_json, override_json};
use super::dispatch::PlanRequest;
use super::types::{DocumentPlan, JsonEntryMode, JsonEntryPlan, JsonPlan};

pub(crate) fn zcode_plans(
    paths: &ConfigurationPaths,
    request: PlanRequest<'_>,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let path = &paths.zcode_provider_path;
    if !path.is_absolute() {
        return Err(ConfigurationError::ReadDocument {
            path: path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ZCode provider configuration path must be absolute",
            ),
        });
    }
    if let Some(contents) = super::super::documents::read_optional(path)? {
        let current = super::super::documents::parse_json_document(&contents, path, false)?;
        if current
            .get("schemaVersion")
            .is_some_and(|version| version != &json!(1))
        {
            return Err(ConfigurationError::UnmanagedDocumentConflict(path.clone()));
        }
    }
    let provider = nan_harness_runtime::zcode_provider_config(
        request.models,
        request.base_url,
        request.api_key,
        request.default_model,
    );
    let mut entries = vec![
        override_json(&["schemaVersion"], json!(1)),
        member(
            &["config", "providerConfigRules", "providerRules"],
            &[("providerId", "nan")],
            provider["config"]["providerConfigRules"]["providerRules"][0].clone(),
        ),
        override_json(
            &["config", "defaultModelSelection"],
            provider["config"]["defaultModelSelection"].clone(),
        ),
    ];
    let mut manual_rules = override_json(
        &["config", "modelConfigRules", "manualProviderModelRules"],
        json!([]),
    );
    manual_rules.mode = JsonEntryMode::EnsureArray;
    entries.push(manual_rules);
    for rule in provider["config"]["modelConfigRules"]["providerModelRules"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(model) = rule["modelId"].as_str() {
            entries.push(member(
                &["config", "modelConfigRules", "providerModelRules"],
                &[("providerId", "nan"), ("modelId", model)],
                rule.clone(),
            ));
        }
    }
    let mut search = Vec::new();
    if request.search.managed {
        search.push(override_json(&["features", "mcp"], json!(true)));
        search.push(exclusive_json(&["mcp", "servers", SEARCH_MCP_ID], json!({"type": "stdio", "command": "nan-harness", "args": ["__search-mcp"], "enabled": true})));
    }
    Ok(vec![
        DocumentPlan::Json(JsonPlan {
            path: path.clone(),
            entries,
        }),
        DocumentPlan::Json(JsonPlan {
            path: paths.home_directory.join(".zcode/cli/config.json"),
            entries: search,
        }),
    ])
}

fn member(path: &[&str], selector: &[(&str, &str)], value: serde_json::Value) -> JsonEntryPlan {
    let mut entry = exclusive_json(path, value);
    entry.selector = Some(
        selector
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    );
    entry
}
