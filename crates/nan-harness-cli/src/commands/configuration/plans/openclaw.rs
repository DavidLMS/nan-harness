use super::super::{CodingModelProfile, Value, json};
use super::combinators::{append_unique_json, exclusive_json, override_json};
use super::search::openclaw_search_plugin;
use super::types::{DocumentPlan, ExactFilePlan, JsonPlan};
use super::values::{openclaw_aliases, openclaw_provider};
use std::path::Path;

pub(crate) fn openclaw_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    let plugin_directory = directory.join("extensions/nan-harness-search");
    let mut entries = vec![
        exclusive_json(
            &["models", "providers", "nan"],
            openclaw_provider(api_key, base_url, models),
        ),
        override_json(
            &["agents", "defaults", "model", "primary"],
            Value::String(format!("nan/{default_model}")),
        ),
        override_json(&["agents", "defaults", "models"], openclaw_aliases(models)),
        override_json(&["models", "mode"], Value::String("merge".to_owned())),
    ];
    if search_managed {
        entries.extend([
            append_unique_json(
                &["plugins", "load", "paths"],
                Value::String(plugin_directory.to_string_lossy().into_owned()),
            ),
            exclusive_json(
                &["plugins", "entries", "nan-harness-search"],
                json!({"enabled": true}),
            ),
            override_json(&["tools", "web", "search", "enabled"], Value::Bool(true)),
            override_json(
                &["tools", "web", "search", "provider"],
                Value::String("nan-harness".to_owned()),
            ),
        ]);
    }
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("openclaw.json"),
            entries,
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("package.json"),
            payload: search_managed.then(|| br#"{"name":"nan-harness-search","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("openclaw.plugin.json"),
            payload: search_managed.then(|| br#"{"id":"nan-harness-search","activation":{"onStartup":false},"contracts":{"webSearchProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("index.js"),
            payload: search_managed.then(|| openclaw_search_plugin().into_bytes()),
        }),
    ]
}
