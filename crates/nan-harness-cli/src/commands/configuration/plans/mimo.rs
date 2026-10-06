use super::super::{ConfigurationError, ConfigurationPaths, SEARCH_MCP_ID, json};
use super::combinators::{exclusive_json, override_json};
use super::dispatch::PlanRequest;
use super::types::{DocumentPlan, JsonPlan};
use std::path::{Path, PathBuf};

pub(crate) fn mimo_plans(
    paths: &ConfigurationPaths,
    request: PlanRequest<'_>,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    for path in [&paths.mimo_config_directory, &paths.mimo_auth_path] {
        if !path.is_absolute() {
            return Err(ConfigurationError::ReadDocument {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "MiMo configuration paths must be absolute",
                ),
            });
        }
    }
    let path = mimo_config_path(&paths.mimo_config_directory);
    let selected = format!("nan/{}", request.default_model);
    let models = nan_harness_runtime::opencode_model_catalog(request.models);
    let mut entries = vec![
        override_json(
            &["$schema"],
            json!("https://mimo.xiaomi.com/mimocode/config.json"),
        ),
        exclusive_json(
            &["provider", "nan"],
            json!({
                "npm": "@ai-sdk/openai-compatible",
                "name": "NaN",
                "only_configured_models": true,
                "options": {"baseURL": request.base_url},
                "models": models,
            }),
        ),
        override_json(&["model"], json!(selected)),
        override_json(&["small_model"], json!(selected)),
        override_json(&["vision_model"], json!(selected)),
    ];
    for tier in ["ultra", "standard", "lite"] {
        entries.push(override_json(&["model_groups", tier], json!(selected)));
    }
    entries.extend(provider_filters(&paths.mimo_config_directory, &path)?);
    if request.search.managed {
        entries.push(exclusive_json(
            &["mcp", SEARCH_MCP_ID],
            json!({
                "type": "local",
                "command": ["nan-harness", "__search-mcp"],
                "enabled": true,
            }),
        ));
    }
    Ok(vec![
        DocumentPlan::Json(JsonPlan {
            path: paths.mimo_auth_path.clone(),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type": "api", "key": request.api_key}),
            )],
        }),
        DocumentPlan::Jsonc(JsonPlan { path, entries }),
    ])
}

fn mimo_config_path(directory: &Path) -> PathBuf {
    // Match MiMo's global merge precedence, including its legacy config filename.
    for name in ["mimocode.jsonc", "mimocode.json", "config.json"] {
        let path = directory.join(name);
        if path.exists() {
            return path;
        }
    }
    directory.join("mimocode.jsonc")
}

fn provider_filters(
    directory: &Path,
    path: &Path,
) -> Result<Vec<super::types::JsonEntryPlan>, ConfigurationError> {
    use super::super::documents::{parse_json_document, read_optional};
    let mut document = json!({});
    for name in ["config.json", "mimocode.json", "mimocode.jsonc"] {
        let source = directory.join(name);
        let Some(contents) = read_optional(&source)? else {
            continue;
        };
        let layer = parse_json_document(&contents, &source, true)?;
        if !layer.is_object() {
            return Err(ConfigurationError::DocumentRootNotObject(source));
        }
        // A lower-priority provider could retain inline credentials through MiMo's deep merge.
        if source != path && layer["provider"].get("nan").is_some() {
            return Err(ConfigurationError::UnmanagedDocumentConflict(source));
        }
        for field in ["enabled_providers", "disabled_providers"] {
            if let Some(value) = layer.get(field) {
                document[field] = value.clone();
            }
        }
    }
    let mut entries = Vec::new();
    if let Some(enabled) = document.get("enabled_providers") {
        let mut enabled = enabled
            .as_array()
            .ok_or_else(|| ConfigurationError::DocumentFieldNotArray {
                path: path.to_path_buf(),
                field: "enabled_providers".to_owned(),
            })?
            .clone();
        if !enabled.contains(&json!("nan")) {
            enabled.push(json!("nan"));
        }
        entries.push(override_json(&["enabled_providers"], json!(enabled)));
    }
    if let Some(disabled) = document.get("disabled_providers") {
        let disabled =
            disabled
                .as_array()
                .ok_or_else(|| ConfigurationError::DocumentFieldNotArray {
                    path: path.to_path_buf(),
                    field: "disabled_providers".to_owned(),
                })?;
        entries.push(override_json(
            &["disabled_providers"],
            json!(
                disabled
                    .iter()
                    .filter(|provider| provider.as_str() != Some("nan"))
                    .collect::<Vec<_>>()
            ),
        ));
    }
    Ok(entries)
}
