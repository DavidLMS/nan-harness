use super::super::{
    CodingModelProfile, ConfigurationError, OMP_SEARCH_EXTENSION_FILE, OmpSearchMode,
    PI_SEARCH_EXTENSION_FILE, PiSearchMode, Value, WebSearchPolicy, YamlValue, json,
    render_omp_search_extension, render_pi_search_extension,
};
use super::combinators::{exclusive_json, override_json, preferred_yaml_path, to_yaml_value};
use super::search::search_mcp_plan;
use super::types::{DocumentPlan, ExactFilePlan, JsonPlan, YamlEntryMode, YamlEntryPlan, YamlPlan};
use super::values::{omp_provider, pi_provider};
use std::path::Path;

pub(crate) fn pi_family_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: super::super::ManagedSearchStatus,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("models.json"),
            entries: vec![exclusive_json(
                &["providers", "nan"],
                pi_provider(base_url, models),
            )],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("auth.json"),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type": "api_key", "key": api_key}),
            )],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("settings.json"),
            entries: vec![
                override_json(&["defaultProvider"], Value::String("nan".to_owned())),
                override_json(&["defaultModel"], Value::String(default_model.to_owned())),
            ],
        }),
        search_mcp_plan(directory.join("mcp.json"), api_key, base_url, false),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join(PI_SEARCH_EXTENSION_FILE),
            payload: search.managed.then(|| {
                render_pi_search_extension(
                    base_url,
                    if search.policy == WebSearchPolicy::Force {
                        PiSearchMode::Force
                    } else {
                        PiSearchMode::Auto
                    },
                )
                .into_bytes()
            }),
        }),
    ]
}

pub(crate) fn omp_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: super::super::ManagedSearchStatus,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let models_path = preferred_yaml_path(directory, "models.yml", "models.yaml");
    let config_path = preferred_yaml_path(directory, "config.yml", "config.yaml");
    Ok(vec![
        DocumentPlan::Yaml(YamlPlan {
            path: models_path,
            entries: vec![YamlEntryPlan {
                path: vec!["providers".to_owned(), "nan".to_owned()],
                value: to_yaml_value(omp_provider(api_key, base_url, models))?,
                mode: YamlEntryMode::Exclusive,
            }],
            legacy_block: None,
        }),
        DocumentPlan::Yaml(YamlPlan {
            path: config_path,
            entries: [
                "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
                "advisor",
            ]
            .into_iter()
            .map(|role| YamlEntryPlan {
                path: vec!["modelRoles".to_owned(), role.to_owned()],
                value: YamlValue::String(format!("nan/{default_model}")),
                mode: YamlEntryMode::Override,
            })
            .collect(),
            legacy_block: None,
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join(OMP_SEARCH_EXTENSION_FILE),
            payload: search.managed.then(|| {
                render_omp_search_extension(
                    base_url,
                    if search.policy == WebSearchPolicy::Force {
                        OmpSearchMode::Force
                    } else {
                        OmpSearchMode::Auto
                    },
                )
                .into_bytes()
            }),
        }),
    ])
}
