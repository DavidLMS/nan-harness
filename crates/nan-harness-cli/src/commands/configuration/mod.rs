mod command;
mod documents;
mod error;

pub(crate) use command::run;
use documents::{
    apply_prepared, document_is_active, dotenv_quote, prepare_documents, prepare_removals,
    rollback_prepared, sha256, yaml_quote,
};
pub(crate) use error::ConfigurationError;

use crate::commands::persistence::{
    IntegrationChange, PersistenceError, PersistenceManager, PersistentIntegration, RemovalOutcome,
    config_directory, write_private_file,
};
use nan_harness_core::{CodingModelProfile, HarnessKind, ReasoningPolicy};
use nan_harness_runtime::ResolvedConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, Permissions};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

const STATE_SCHEMA_VERSION: u8 = 1;
const STATE_FILE_NAME: &str = "configurations.json";
const DEFAULT_MODEL_ID: &str = "qwen3.6";
const SUPPORTED_HARNESSES: [HarnessKind; 11] = [
    HarnessKind::OpenCode,
    HarnessKind::Hermes,
    HarnessKind::Pi,
    HarnessKind::PrimeAgent,
    HarnessKind::DeepSeekHarness,
    HarnessKind::OpenClaw,
    HarnessKind::Cline,
    HarnessKind::QwenCode,
    HarnessKind::KimiCode,
    HarnessKind::Aider,
    HarnessKind::Goose,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationState {
    schema_version: u8,
    #[serde(default)]
    harnesses: BTreeMap<String, HarnessReceipt>,
}

impl Default for ConfigurationState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            harnesses: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HarnessReceipt {
    credential_fingerprint: String,
    model_ids: Vec<String>,
    documents: Vec<DocumentReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "format", rename_all = "kebab-case")]
enum DocumentReceipt {
    Json(JsonReceipt),
    TextBlock(TextBlockReceipt),
    ExactFile(ExactFileReceipt),
    Toml(TomlReceipt),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonReceipt {
    path: PathBuf,
    created_file: bool,
    entries: Vec<JsonEntryReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JsonEntryReceipt {
    path: Vec<String>,
    value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TextBlockReceipt {
    path: PathBuf,
    created_file: bool,
    begin: String,
    end: String,
    block_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExactFileReceipt {
    path: PathBuf,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TomlReceipt {
    path: PathBuf,
    created_file: bool,
    provider_sha256: String,
    models: BTreeMap<String, String>,
    default_model_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_default_model: Option<String>,
}

#[derive(Debug, Clone)]
enum DocumentPlan {
    Json(JsonPlan),
    TextBlock(TextBlockPlan),
    ExactFile(ExactFilePlan),
    Kimi(KimiPlan),
}

#[derive(Debug, Clone)]
struct JsonPlan {
    path: PathBuf,
    entries: Vec<JsonEntryPlan>,
}

#[derive(Debug, Clone)]
struct JsonEntryPlan {
    path: Vec<String>,
    value: Value,
    mode: JsonEntryMode,
}

#[derive(Debug, Clone, Copy)]
enum JsonEntryMode {
    Exclusive,
    Override,
}

#[derive(Debug, Clone)]
struct TextBlockPlan {
    path: PathBuf,
    begin: String,
    end: String,
    body: String,
    conflicting_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct ExactFilePlan {
    path: PathBuf,
    payload: Vec<u8>,
}

#[derive(Debug, Clone)]
struct KimiPlan {
    path: PathBuf,
    api_key: String,
    base_url: String,
    models: Vec<CodingModelProfile>,
    default_model: String,
}

struct PreparedDocument {
    path: PathBuf,
    original: Option<Vec<u8>>,
    permissions: Option<Permissions>,
    replacement: Option<Vec<u8>>,
    receipt: DocumentReceipt,
}

#[derive(Debug)]
pub(crate) struct ConfigurationManager {
    state_path: PathBuf,
    home_directory: PathBuf,
    prime_directory: PathBuf,
    qwen_directory: PathBuf,
    deepseek_directory: PathBuf,
    kimi_directory: PathBuf,
    opencode_auth_path: PathBuf,
    goose_directory: PathBuf,
    legacy: PersistenceManager,
}

impl ConfigurationManager {
    pub(crate) fn from_environment() -> Result<Self, ConfigurationError> {
        let state_directory =
            config_directory().ok_or(ConfigurationError::MissingStateDirectory)?;
        let home_directory = home_directory().ok_or(ConfigurationError::MissingHomeDirectory)?;
        let prime_directory = env::var_os("PRIME_AGENT_CODING_AGENT_DIR")
            .map_or_else(|| home_directory.join(".prime/agent"), PathBuf::from);
        let qwen_directory =
            env::var_os("QWEN_HOME").map_or_else(|| home_directory.join(".qwen"), PathBuf::from);
        let deepseek_directory =
            env::var_os("DSH_HOME").map_or_else(|| home_directory.join(".dsh"), PathBuf::from);
        let kimi_directory = env::var_os("KIMI_CODE_HOME")
            .map_or_else(|| home_directory.join(".kimi-code"), PathBuf::from);
        let opencode_auth_path = opencode_auth_path(&home_directory);
        let goose_directory = goose_config_directory(&home_directory);
        Ok(Self {
            state_path: state_directory.join(STATE_FILE_NAME),
            prime_directory,
            qwen_directory,
            deepseek_directory,
            kimi_directory,
            opencode_auth_path,
            goose_directory,
            home_directory,
            legacy: PersistenceManager::from_environment()?,
        })
    }

    #[cfg(test)]
    fn new(state_directory: &Path, home_directory: &Path) -> Self {
        Self {
            state_path: state_directory.join(STATE_FILE_NAME),
            home_directory: home_directory.to_path_buf(),
            prime_directory: home_directory.join(".prime/agent"),
            qwen_directory: home_directory.join(".qwen"),
            deepseek_directory: home_directory.join(".dsh"),
            kimi_directory: home_directory.join(".kimi-code"),
            opencode_auth_path: home_directory.join(".local/share/opencode/auth.json"),
            goose_directory: home_directory.join(".config/goose"),
            legacy: PersistenceManager::new_for_tests(state_directory, home_directory),
        }
    }

    pub(crate) fn configured_harnesses(&self) -> Result<Vec<HarnessKind>, ConfigurationError> {
        let state = self.load_state()?;
        let mut configured = state
            .harnesses
            .keys()
            .filter_map(|value| value.parse::<HarnessKind>().ok())
            .collect::<BTreeSet<_>>();
        for integration in self.legacy.configured_integrations()? {
            configured.insert(legacy_harness(integration));
        }
        Ok(configured.into_iter().collect())
    }

    pub(crate) fn is_configured(&self, harness: HarnessKind) -> Result<bool, ConfigurationError> {
        Ok(self.configured_harnesses()?.contains(&harness))
    }

    pub(crate) fn is_active(&self, harness: HarnessKind) -> Result<bool, ConfigurationError> {
        let state = self.load_state()?;
        let Some(receipt) = state.harnesses.get(&harness.to_string()) else {
            return Ok(self.legacy_is_active(harness));
        };
        Ok(receipt.documents.iter().all(document_is_active) && self.legacy_is_active(harness))
    }

    pub(crate) fn credential_is_current(
        &self,
        harness: HarnessKind,
        saved_fingerprint: Option<&str>,
    ) -> Result<Option<bool>, ConfigurationError> {
        let state = self.load_state()?;
        Ok(state.harnesses.get(&harness.to_string()).map(|receipt| {
            saved_fingerprint
                .is_some_and(|fingerprint| fingerprint == receipt.credential_fingerprint)
        }))
    }

    pub(crate) fn configure(
        &self,
        harness: HarnessKind,
        config: &ResolvedConfig,
        models: &[CodingModelProfile],
    ) -> Result<ConfigurationChange, ConfigurationError> {
        ensure_supported(harness)?;
        let default_model = preferred_model(models);
        let (api_key, fingerprint) = config
            .secrets
            .with_secret(&config.provider_credential_ref, |value| {
                (value.to_owned(), sha256(value.as_bytes()))
            })
            .map_err(PersistenceError::Secret)?;
        let mut state = self.load_state()?;
        let previous = state.harnesses.get(&harness.to_string());
        let plans = self.plans_for(
            harness,
            &api_key,
            &config.provider_base_url,
            models,
            default_model,
        )?;
        let prepared =
            prepare_documents(&plans, previous.map(|receipt| receipt.documents.as_slice()))?;
        apply_prepared(&prepared)?;
        let catalog_change =
            match self.configure_catalogs(harness, models, &config.provider_base_url) {
                Ok(change) => change,
                Err(error) => {
                    rollback_prepared(&prepared);
                    return Err(error);
                }
            };
        let changed = catalog_change.as_ref().is_some_and(|change| change.changed)
            || prepared
                .iter()
                .any(|document| document.original.as_deref() != document.replacement.as_deref());
        let mut paths = prepared
            .iter()
            .map(|document| document.path.clone())
            .collect::<Vec<_>>();
        if let Some(change) = catalog_change {
            paths.push(change.path);
            paths.extend(change.additional_paths);
        }
        paths.sort();
        paths.dedup();
        state.harnesses.insert(
            harness.to_string(),
            HarnessReceipt {
                credential_fingerprint: fingerprint,
                model_ids: models.iter().map(|model| model.id.clone()).collect(),
                documents: prepared
                    .iter()
                    .map(|document| document.receipt.clone())
                    .collect(),
            },
        );
        if let Err(error) = self.save_state(&state) {
            rollback_prepared(&prepared);
            return Err(error);
        }
        Ok(ConfigurationChange {
            changed,
            paths,
            model_count: models.len(),
        })
    }

    pub(crate) fn remove(
        &self,
        harness: HarnessKind,
    ) -> Result<RemovalOutcome, ConfigurationError> {
        ensure_supported(harness)?;
        let mut state = self.load_state()?;
        let Some(receipt) = state.harnesses.get(&harness.to_string()).cloned() else {
            return self.remove_legacy(harness);
        };
        let prepared = prepare_removals(&receipt.documents)?;
        self.remove_legacy(harness)?;
        apply_prepared(&prepared)?;
        state.harnesses.remove(&harness.to_string());
        if let Err(error) = self.save_state(&state) {
            rollback_prepared(&prepared);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn remove_all(
        &self,
    ) -> Result<Vec<(HarnessKind, RemovalOutcome)>, ConfigurationError> {
        self.configured_harnesses()?
            .into_iter()
            .map(|harness| self.remove(harness).map(|outcome| (harness, outcome)))
            .collect()
    }

    pub(crate) fn paths_for(
        &self,
        harness: HarnessKind,
    ) -> Result<Vec<PathBuf>, ConfigurationError> {
        ensure_supported(harness)?;
        let placeholder_models = vec![CodingModelProfile::generic(DEFAULT_MODEL_ID)];
        let mut paths = self
            .plans_for(
                harness,
                "<saved API key>",
                "https://api.nan.builders/v1",
                &placeholder_models,
                DEFAULT_MODEL_ID,
            )?
            .into_iter()
            .map(|plan| match plan {
                DocumentPlan::Json(plan) => plan.path,
                DocumentPlan::TextBlock(plan) => plan.path,
                DocumentPlan::ExactFile(plan) => plan.path,
                DocumentPlan::Kimi(plan) => plan.path,
            })
            .collect::<Vec<_>>();
        if let Some(integration) = catalog_integration(harness) {
            paths.extend(self.legacy.managed_catalog_paths(integration)?);
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    fn plans_for(
        &self,
        harness: HarnessKind,
        api_key: &str,
        base_url: &str,
        models: &[CodingModelProfile],
        default_model: &str,
    ) -> Result<Vec<DocumentPlan>, ConfigurationError> {
        let plans = match harness {
            HarnessKind::OpenCode => vec![DocumentPlan::Json(JsonPlan {
                path: self.opencode_auth_path.clone(),
                entries: vec![exclusive_json(
                    &["nan"],
                    json!({"type": "api", "key": api_key}),
                )],
            })],
            HarnessKind::Pi => pi_family_plans(
                &self.home_directory.join(".pi/agent"),
                api_key,
                base_url,
                models,
                default_model,
            ),
            HarnessKind::PrimeAgent => pi_family_plans(
                &self.prime_directory,
                api_key,
                base_url,
                models,
                default_model,
            ),
            HarnessKind::QwenCode => vec![DocumentPlan::TextBlock(TextBlockPlan {
                path: self.qwen_directory.join(".env"),
                begin: "# nan-harness:begin provider-credential".to_owned(),
                end: "# nan-harness:end provider-credential".to_owned(),
                body: format!("NAN_API_KEY={}", dotenv_quote(api_key)),
                conflicting_keys: vec!["NAN_API_KEY=".to_owned()],
            })],
            HarnessKind::DeepSeekHarness => vec![DocumentPlan::TextBlock(TextBlockPlan {
                path: self.deepseek_directory.join(".credentials.yaml"),
                begin: "# nan-harness:begin provider-credential".to_owned(),
                end: "# nan-harness:end provider-credential".to_owned(),
                body: format!("NAN_API_KEY: {}", yaml_quote(api_key)?),
                conflicting_keys: vec!["NAN_API_KEY:".to_owned()],
            })],
            HarnessKind::Aider => vec![DocumentPlan::TextBlock(TextBlockPlan {
                path: self.home_directory.join(".aider.conf.yml"),
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
                body: format!(
                    "api-key:\n  - {}\nmodel: {}",
                    yaml_quote(&format!("nan={api_key}"))?,
                    yaml_quote(&format!("nan/{default_model}"))?
                ),
                conflicting_keys: vec!["api-key:".to_owned(), "model:".to_owned()],
            })],
            HarnessKind::Hermes => vec![DocumentPlan::TextBlock(TextBlockPlan {
                path: self.home_directory.join(".hermes/config.yaml"),
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
                body: format!(
                    "model:\n  default: {}\n  provider: custom\n  base_url: {}\n  api_key: {}",
                    yaml_quote(default_model)?,
                    yaml_quote(base_url)?,
                    yaml_quote(api_key)?
                ),
                conflicting_keys: vec!["model:".to_owned()],
            })],
            HarnessKind::OpenClaw => vec![DocumentPlan::Json(JsonPlan {
                path: self.home_directory.join(".openclaw/openclaw.json"),
                entries: vec![
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
                ],
            })],
            HarnessKind::Cline => cline_plans(
                &self.home_directory.join(".cline/data/settings"),
                api_key,
                base_url,
                models,
                default_model,
            ),
            HarnessKind::KimiCode => vec![DocumentPlan::Kimi(KimiPlan {
                path: self.kimi_directory.join("config.toml"),
                api_key: api_key.to_owned(),
                base_url: base_url.to_owned(),
                models: models.to_vec(),
                default_model: default_model.to_owned(),
            })],
            HarnessKind::Goose => goose_plans(
                &self.goose_directory,
                api_key,
                base_url,
                models,
                default_model,
            )?,
            HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => {
                return Err(ConfigurationError::BridgeOnly(harness));
            }
        };
        Ok(plans)
    }

    fn configure_catalogs(
        &self,
        harness: HarnessKind,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<Option<IntegrationChange>, ConfigurationError> {
        let change = match harness {
            HarnessKind::OpenCode => {
                Some(self.legacy.configure_opencode(models, provider_base_url)?)
            }
            HarnessKind::QwenCode => {
                Some(self.legacy.configure_qwen_code(models, provider_base_url)?)
            }
            HarnessKind::DeepSeekHarness => Some(
                self.legacy
                    .configure_deepseek_harness(models, provider_base_url)?,
            ),
            HarnessKind::Aider => Some(self.legacy.configure_aider(models, provider_base_url)?),
            _ => None,
        };
        Ok(change)
    }

    fn remove_legacy(&self, harness: HarnessKind) -> Result<RemovalOutcome, ConfigurationError> {
        let outcome = match harness {
            HarnessKind::OpenCode => self.legacy.unpersist_opencode()?,
            HarnessKind::Pi => self.legacy.unpersist_pi()?,
            HarnessKind::PrimeAgent => self.legacy.unpersist_prime_agent()?,
            HarnessKind::QwenCode => self.legacy.unpersist_qwen_code()?,
            HarnessKind::DeepSeekHarness => self.legacy.unpersist_deepseek_harness()?,
            HarnessKind::Aider => self.legacy.unpersist_aider()?,
            _ => RemovalOutcome::NotConfigured,
        };
        Ok(outcome)
    }

    fn legacy_is_active(&self, harness: HarnessKind) -> bool {
        match harness {
            HarnessKind::OpenCode => self.legacy.opencode_is_active(),
            HarnessKind::QwenCode => self.legacy.qwen_code_is_active(),
            HarnessKind::DeepSeekHarness => self.legacy.deepseek_harness_is_active(),
            HarnessKind::Aider => self.legacy.aider_is_active(),
            _ => true,
        }
    }

    fn load_state(&self) -> Result<ConfigurationState, ConfigurationError> {
        match fs::read(&self.state_path) {
            Ok(contents) => {
                let state: ConfigurationState =
                    serde_json::from_slice(&contents).map_err(ConfigurationError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(ConfigurationError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConfigurationState::default())
            }
            Err(source) => Err(ConfigurationError::ReadState {
                path: self.state_path.clone(),
                source,
            }),
        }
    }

    fn save_state(&self, state: &ConfigurationState) -> Result<(), ConfigurationError> {
        let payload =
            serde_json::to_vec_pretty(state).map_err(ConfigurationError::SerializeState)?;
        write_private_file(&self.state_path, &payload, None)?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct ConfigurationChange {
    changed: bool,
    paths: Vec<PathBuf>,
    model_count: usize,
}

fn pi_family_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
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
    ]
}

fn cline_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
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
    ]
}

fn goose_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
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
            payload: provider,
        }),
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join("secrets.yaml"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: format!("NAN_HARNESS_API_KEY: {}", yaml_quote(api_key)?),
            conflicting_keys: vec!["NAN_HARNESS_API_KEY:".to_owned()],
        }),
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join("config.yaml"),
            begin: "# nan-harness:begin provider-defaults".to_owned(),
            end: "# nan-harness:end provider-defaults".to_owned(),
            body: format!(
                "GOOSE_PROVIDER: nan_harness\nGOOSE_MODEL: {}",
                yaml_quote(default_model)?
            ),
            conflicting_keys: vec!["GOOSE_PROVIDER:".to_owned(), "GOOSE_MODEL:".to_owned()],
        }),
    ])
}

fn pi_provider(base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": "NAN_API_KEY",
        "models": models.iter().map(pi_model).collect::<Vec<_>>()
    })
}

fn pi_model(model: &CodingModelProfile) -> Value {
    json!({
        "id": model.id,
        "name": model.display_name,
        "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
        "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window,
        "maxTokens": model.max_output_tokens,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": matches!(model.reasoning, ReasoningPolicy::Effort { .. }),
            "maxTokensField": "max_tokens"
        }
    })
}

fn openclaw_provider(api_key: &str, base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "api": "openai-completions",
        "apiKey": api_key,
        "baseUrl": base_url,
        "models": models.iter().map(|model| json!({
            "id": model.id,
            "name": model.display_name,
            "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
            "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
            "contextWindow": model.context_window,
            "maxTokens": model.max_output_tokens
        })).collect::<Vec<_>>()
    })
}

fn openclaw_aliases(models: &[CodingModelProfile]) -> Value {
    Value::Object(
        models
            .iter()
            .map(|model| (format!("nan/{}", model.id), json!({})))
            .collect(),
    )
}

fn cline_models(models: &[CodingModelProfile]) -> Value {
    Value::Array(
        models
            .iter()
            .map(|model| {
                json!({
                    "id": model.id,
                    "name": model.display_name,
                    "contextWindow": model.context_window,
                    "maxTokens": model.max_output_tokens,
                    "supportsImages": model.image_input,
                    "supportsReasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown)
                })
            })
            .collect(),
    )
}

fn preferred_model(models: &[CodingModelProfile]) -> &str {
    models
        .iter()
        .find(|model| model.id == DEFAULT_MODEL_ID)
        .or_else(|| models.first())
        .map_or(DEFAULT_MODEL_ID, |model| model.id.as_str())
}

fn exclusive_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::Exclusive,
    }
}

fn override_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::Override,
    }
}

fn ensure_supported(harness: HarnessKind) -> Result<(), ConfigurationError> {
    if SUPPORTED_HARNESSES.contains(&harness) {
        Ok(())
    } else {
        Err(ConfigurationError::BridgeOnly(harness))
    }
}

const fn legacy_harness(integration: PersistentIntegration) -> HarnessKind {
    match integration {
        PersistentIntegration::OpenCode => HarnessKind::OpenCode,
        PersistentIntegration::Pi => HarnessKind::Pi,
        PersistentIntegration::PrimeAgent => HarnessKind::PrimeAgent,
        PersistentIntegration::QwenCode => HarnessKind::QwenCode,
        PersistentIntegration::DeepSeekHarness => HarnessKind::DeepSeekHarness,
        PersistentIntegration::Aider => HarnessKind::Aider,
    }
}

const fn catalog_integration(harness: HarnessKind) -> Option<PersistentIntegration> {
    match harness {
        HarnessKind::OpenCode => Some(PersistentIntegration::OpenCode),
        HarnessKind::QwenCode => Some(PersistentIntegration::QwenCode),
        HarnessKind::DeepSeekHarness => Some(PersistentIntegration::DeepSeekHarness),
        HarnessKind::Aider => Some(PersistentIntegration::Aider),
        _ => None,
    }
}

fn opencode_auth_path(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("opencode/auth.json")
    }
    #[cfg(not(windows))]
    {
        env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local/share"), PathBuf::from)
            .join("opencode/auth.json")
    }
}

fn goose_config_directory(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Block/goose/config")
    }
    #[cfg(not(windows))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("goose")
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

#[cfg(test)]
mod tests;
