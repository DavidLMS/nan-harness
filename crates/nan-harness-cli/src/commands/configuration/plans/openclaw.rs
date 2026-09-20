use super::super::{CodingModelProfile, MediaSelection, Value, json};
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
    media: MediaSelection,
) -> Vec<DocumentPlan> {
    let plugin_directory = directory.join("extensions/nan-harness-search");
    let media_plugin_directory = directory.join("extensions/nan-harness-media");
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
    if media.any() {
        entries.extend([
            append_unique_json(
                &["plugins", "load", "paths"],
                Value::String(media_plugin_directory.to_string_lossy().into_owned()),
            ),
            override_json(
                &["plugins", "entries", "nan-harness-media"],
                json!({"enabled": true}),
            ),
        ]);
    }
    if media.tts {
        entries.push(override_json(
            &["tts", "providers", "nan-harness"],
            json!({"model": "kokoro"}),
        ));
        entries.push(override_json(
            &["tts", "provider"],
            Value::String("nan-harness".to_owned()),
        ));
    }
    if media.stt {
        entries.extend([
            override_json(&["tools", "media", "audio", "enabled"], Value::Bool(true)),
            override_json(
                &["tools", "media", "audio", "models"],
                json!([{"type": "provider", "provider": "nan-harness", "model": "whisper-1"}]),
            ),
        ]);
    }
    if media.image {
        entries.push(override_json(
            &["agents", "defaults", "mediaModels", "image", "primary"],
            Value::String("nan-harness/flux-2-klein".to_owned()),
        ));
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
        DocumentPlan::ExactFile(ExactFilePlan {
            path: media_plugin_directory.join("package.json"),
            payload: media.any().then(|| br#"{"name":"nan-harness-media","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: media_plugin_directory.join("openclaw.plugin.json"),
            payload: media.any().then(|| br#"{"id":"nan-harness-media","activation":{"onStartup":false},"contracts":{"speechProviders":["nan-harness"],"mediaUnderstandingProviders":["nan-harness"],"imageGenerationProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: media_plugin_directory.join("index.js"),
            payload: media
                .any()
                .then(|| nan_harness_adapters::render_openclaw_media_plugin(base_url).into_bytes()),
        }),
    ]
}
