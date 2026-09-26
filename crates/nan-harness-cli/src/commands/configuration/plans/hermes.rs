use super::super::{ConfigurationError, MediaSelection, YamlValue, json};
use super::combinators::to_yaml_value;
use super::search::hermes_search_provider;
use super::types::{
    DocumentPlan, ExactFilePlan, LegacyTextBlock, YamlEntryMode, YamlEntryPlan, YamlPlan,
};
use std::path::Path;

fn media_entries(
    base_url: &str,
    media: MediaSelection,
) -> Result<Vec<YamlEntryPlan>, ConfigurationError> {
    let mut entries = Vec::new();
    if media.stt {
        entries.extend([
            YamlEntryPlan {
                path: vec!["stt".to_owned(), "provider".to_owned()],
                value: YamlValue::String("nan-whisper".to_owned()),
                mode: YamlEntryMode::Override,
            },
            YamlEntryPlan {
                path: vec![
                    "stt".to_owned(),
                    "providers".to_owned(),
                    "nan-whisper".to_owned(),
                ],
                value: to_yaml_value(nan_harness_adapters::hermes_command_provider_config(
                    "stt",
                    "whisper-1",
                    base_url,
                ))?,
                mode: YamlEntryMode::Override,
            },
        ]);
    }
    if media.tts {
        entries.extend([
            YamlEntryPlan {
                path: vec!["tts".to_owned(), "provider".to_owned()],
                value: YamlValue::String("nan-kokoro".to_owned()),
                mode: YamlEntryMode::Override,
            },
            YamlEntryPlan {
                path: vec![
                    "tts".to_owned(),
                    "providers".to_owned(),
                    "nan-kokoro".to_owned(),
                ],
                value: to_yaml_value(nan_harness_adapters::hermes_command_provider_config(
                    "tts", "kokoro", base_url,
                ))?,
                mode: YamlEntryMode::Override,
            },
        ]);
    }
    if media.image {
        entries.extend([
            YamlEntryPlan {
                path: vec!["image_gen".to_owned(), "model".to_owned()],
                value: YamlValue::String(media.image_model.unwrap_or_default().as_str().to_owned()),
                mode: YamlEntryMode::Override,
            },
            YamlEntryPlan {
                path: vec![
                    "plugins".to_owned(),
                    "entries".to_owned(),
                    "image_gen/nan_harness".to_owned(),
                    "allow_tool_override".to_owned(),
                ],
                value: YamlValue::Bool(true),
                mode: YamlEntryMode::Override,
            },
            YamlEntryPlan {
                path: vec!["image_gen".to_owned(), "provider".to_owned()],
                value: YamlValue::String("nan-harness".to_owned()),
                mode: YamlEntryMode::Override,
            },
            YamlEntryPlan {
                path: vec!["plugins".to_owned(), "enabled".to_owned()],
                value: YamlValue::String("image_gen/nan_harness".to_owned()),
                mode: YamlEntryMode::AppendUnique,
            },
        ]);
    }
    Ok(entries)
}

pub(crate) fn hermes_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    default_model: &str,
    search_managed: bool,
    media: MediaSelection,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let mut entries = vec![YamlEntryPlan {
        path: vec!["model".to_owned()],
        value: to_yaml_value(json!({
            "default": default_model,
            "provider": "custom",
            "base_url": base_url,
            "api_key": api_key
        }))?,
        mode: YamlEntryMode::Exclusive,
    }];
    if search_managed {
        entries.extend([
            YamlEntryPlan {
                path: vec!["plugins".to_owned(), "enabled".to_owned()],
                value: YamlValue::String("web/nan_harness".to_owned()),
                mode: YamlEntryMode::AppendUnique,
            },
            YamlEntryPlan {
                path: vec!["web".to_owned(), "search_backend".to_owned()],
                value: YamlValue::String("nan-harness".to_owned()),
                mode: YamlEntryMode::Override,
            },
        ]);
    }
    entries.extend(media_entries(base_url, media)?);
    Ok(vec![
        DocumentPlan::Yaml(YamlPlan {
            path: directory.join("config.yaml"),
            entries,
            legacy_block: Some(LegacyTextBlock {
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
            }),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/__init__.py"),
            payload: search_managed.then(|| b"from .provider import NanHarnessWebSearchProvider\n\n\ndef register(ctx):\n    ctx.register_web_search_provider(NanHarnessWebSearchProvider())\n".to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/provider.py"),
            payload: search_managed.then(|| hermes_search_provider().into_bytes()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/plugin.yaml"),
            payload: search_managed.then(|| b"name: nan-search\nkind: backend\nversion: 1.0.0\ndescription: nan-search\nauthor: NaN\nprovides_web_providers:\n  - nan-harness\n".to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/image_gen/nan_harness/__init__.py"),
            payload: media.image.then(|| b"from .provider import register\n".to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/image_gen/nan_harness/provider.py"),
            payload: media
                .image
                .then(|| nan_harness_adapters::render_hermes_image_plugin(base_url, media.image_model.unwrap_or_default()).into_bytes()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/image_gen/nan_harness/plugin.yaml"),
            payload: media.image.then(|| b"name: nan-image\nkind: backend\nversion: 1.0.0\ndescription: NaN image generation\nauthor: NaN\nprovides_image_gen_providers:\n  - nan-harness\n".to_vec()),
        }),
    ])
}
