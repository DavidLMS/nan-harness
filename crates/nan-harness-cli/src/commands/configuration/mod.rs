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
use nan_harness_adapters::{
    OmpSearchMode, PiSearchMode, render_omp_search_extension, render_pi_search_extension,
};
use nan_harness_core::{
    CodingModelProfile, HarnessKind, ReasoningEffort, ReasoningPolicy, WebSearchPolicy,
};
use nan_harness_runtime::{
    ResolvedConfig, SearchConfiguration, SearchPolicyError, inspect_search_configuration,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use serde_yaml_ng::Value as YamlValue;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, Permissions};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

const STATE_SCHEMA_VERSION: u8 = 1;
const STATE_FILE_NAME: &str = "configurations.json";
const DEFAULT_MODEL_ID: &str = "qwen3.6";
const SEARCH_MCP_ID: &str = "nan-search";
const SEARCH_TOKEN_ENVIRONMENT: &str = "NAN_HARNESS_SEARCH_API_KEY";
const PI_SEARCH_EXTENSION_FILE: &str = "extensions/nan-search.js";
const OMP_SEARCH_EXTENSION_FILE: &str = "extensions/nan-search.mjs";
const SUPPORTED_HARNESSES: [HarnessKind; 12] = [
    HarnessKind::OpenCode,
    HarnessKind::Hermes,
    HarnessKind::Pi,
    HarnessKind::Omp,
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
    #[serde(default)]
    search_policy: WebSearchPolicy,
    #[serde(default)]
    search_managed: bool,
    documents: Vec<DocumentReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "format", rename_all = "kebab-case")]
enum DocumentReceipt {
    Json(JsonReceipt),
    Yaml(YamlReceipt),
    TextBlock(TextBlockReceipt),
    ExactFile(ExactFileReceipt),
    Toml(TomlReceipt),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct YamlReceipt {
    path: PathBuf,
    created_file: bool,
    entries: Vec<YamlEntryReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct YamlEntryReceipt {
    path: Vec<String>,
    value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<YamlValue>,
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
    #[serde(default = "default_true")]
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExactFileReceipt {
    path: PathBuf,
    sha256: String,
    #[serde(default = "default_true")]
    active: bool,
}

const fn default_true() -> bool {
    true
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
    Yaml(YamlPlan),
    TextBlock(TextBlockPlan),
    ExactFile(ExactFilePlan),
    Kimi(KimiPlan),
}

#[derive(Debug, Clone)]
struct YamlPlan {
    path: PathBuf,
    entries: Vec<YamlEntryPlan>,
    legacy_block: Option<LegacyTextBlock>,
}

#[derive(Debug, Clone)]
struct YamlEntryPlan {
    path: Vec<String>,
    value: YamlValue,
    mode: YamlEntryMode,
}

#[derive(Debug, Clone, Copy)]
enum YamlEntryMode {
    Exclusive,
    Override,
    AppendUnique,
}

#[derive(Debug, Clone)]
struct LegacyTextBlock {
    begin: String,
    end: String,
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
    AppendUnique,
}

#[derive(Debug, Clone)]
struct TextBlockPlan {
    path: PathBuf,
    begin: String,
    end: String,
    body: Option<String>,
    conflicting_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct ExactFilePlan {
    path: PathBuf,
    payload: Option<Vec<u8>>,
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
    omp_directory: PathBuf,
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
        let omp_directory = env::var_os("PI_CODING_AGENT_DIR")
            .map_or_else(|| home_directory.join(".omp/agent"), PathBuf::from);
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
            omp_directory,
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
            omp_directory: home_directory.join(".omp/agent"),
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

    fn search_status(
        &self,
        harness: HarnessKind,
    ) -> Result<Option<ManagedSearchStatus>, ConfigurationError> {
        let state = self.load_state()?;
        Ok(state
            .harnesses
            .get(&harness.to_string())
            .map(|receipt| ManagedSearchStatus {
                policy: receipt.search_policy,
                managed: receipt.search_managed,
            }))
    }

    pub(crate) fn configure(
        &self,
        harness: HarnessKind,
        config: &ResolvedConfig,
        models: &[CodingModelProfile],
        search_policy_override: Option<WebSearchPolicy>,
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
        let search_policy = search_policy_override
            .or_else(|| previous.map(|receipt| receipt.search_policy))
            .unwrap_or_default();
        let search_managed = self.resolve_managed_search(
            harness,
            search_policy,
            previous.is_some_and(|receipt| receipt.search_managed),
        )?;
        let plans = self.plans_for(
            harness,
            &api_key,
            &config.provider_base_url,
            models,
            default_model,
            ManagedSearchStatus {
                policy: search_policy,
                managed: search_managed,
            },
        )?;
        let prepared =
            prepare_documents(&plans, previous.map(|receipt| receipt.documents.as_slice()))?;
        apply_prepared(&prepared)?;
        let catalog_change = match self.configure_catalogs(
            harness,
            models,
            &config.provider_base_url,
            &api_key,
            search_managed,
        ) {
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
            .filter(|document| receipt_manages_content(&document.receipt))
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
                search_policy,
                search_managed,
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
            search: ManagedSearchStatus {
                policy: search_policy,
                managed: search_managed,
            },
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

    fn paths_for_search(
        &self,
        harness: HarnessKind,
        search_managed: bool,
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
                ManagedSearchStatus {
                    policy: WebSearchPolicy::Auto,
                    managed: search_managed,
                },
            )?
            .into_iter()
            .filter_map(|plan| match plan {
                DocumentPlan::Json(plan) if plan.entries.is_empty() => None,
                DocumentPlan::Json(plan) => Some(plan.path),
                DocumentPlan::Yaml(plan) => Some(plan.path),
                DocumentPlan::TextBlock(plan) if plan.body.is_none() => None,
                DocumentPlan::TextBlock(plan) => Some(plan.path),
                DocumentPlan::ExactFile(plan) if plan.payload.is_none() => None,
                DocumentPlan::ExactFile(plan) => Some(plan.path),
                DocumentPlan::Kimi(plan) => Some(plan.path),
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
        search: ManagedSearchStatus,
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
                search,
            ),
            HarnessKind::Omp => omp_plans(
                &self.omp_directory,
                api_key,
                base_url,
                models,
                default_model,
                search,
            )?,
            HarnessKind::PrimeAgent => pi_family_plans(
                &self.prime_directory,
                api_key,
                base_url,
                models,
                default_model,
                search,
            ),
            HarnessKind::QwenCode => {
                qwen_plans(&self.qwen_directory, api_key, base_url, search.managed)
            }
            HarnessKind::DeepSeekHarness => {
                deepseek_plans(&self.deepseek_directory, api_key, base_url, search.managed)?
            }
            HarnessKind::Aider => vec![DocumentPlan::TextBlock(TextBlockPlan {
                path: self.home_directory.join(".aider.conf.yml"),
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
                body: Some(format!(
                    "api-key:\n  - {}\nmodel: {}",
                    yaml_quote(&format!("nan={api_key}"))?,
                    yaml_quote(&format!("nan/{default_model}"))?
                )),
                conflicting_keys: vec!["api-key:".to_owned(), "model:".to_owned()],
            })],
            HarnessKind::Hermes => hermes_plans(
                &self.home_directory.join(".hermes"),
                api_key,
                base_url,
                default_model,
                search.managed,
            )?,
            HarnessKind::OpenClaw => openclaw_plans(
                &self.home_directory.join(".openclaw"),
                api_key,
                base_url,
                models,
                default_model,
                search.managed,
            ),
            HarnessKind::Cline => cline_plans(
                &self.home_directory.join(".cline/data/settings"),
                api_key,
                base_url,
                models,
                default_model,
                search.managed,
            ),
            HarnessKind::KimiCode => vec![
                DocumentPlan::Kimi(KimiPlan {
                    path: self.kimi_directory.join("config.toml"),
                    api_key: api_key.to_owned(),
                    base_url: base_url.to_owned(),
                    models: models.to_vec(),
                    default_model: default_model.to_owned(),
                }),
                search_mcp_plan(
                    self.kimi_directory.join("mcp.json"),
                    api_key,
                    base_url,
                    search.managed,
                ),
            ],
            HarnessKind::Goose => goose_plans(
                &self.goose_directory,
                api_key,
                base_url,
                models,
                default_model,
                search.managed,
            )?,
            HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => {
                return Err(ConfigurationError::BridgeOnly(harness));
            }
        };
        Ok(plans)
    }

    fn resolve_managed_search(
        &self,
        harness: HarnessKind,
        policy: WebSearchPolicy,
        previously_managed: bool,
    ) -> Result<bool, ConfigurationError> {
        if policy == WebSearchPolicy::Disabled {
            return Ok(false);
        }
        if harness == HarnessKind::Aider {
            return if policy == WebSearchPolicy::Force {
                Err(SearchPolicyError::UnsupportedHarness(harness).into())
            } else {
                Ok(false)
            };
        }
        if matches!(
            harness,
            HarnessKind::Pi | HarnessKind::Omp | HarnessKind::PrimeAgent
        ) {
            return Ok(true);
        }
        let working_directory = env::current_dir().map_err(ConfigurationError::CurrentDirectory)?;
        let detected =
            inspect_search_configuration(harness, &self.home_directory, &working_directory)?;
        Ok(match (policy, detected) {
            (WebSearchPolicy::Force | WebSearchPolicy::Auto, SearchConfiguration::ManagedNan) => {
                previously_managed
            }
            (WebSearchPolicy::Force, _) | (WebSearchPolicy::Auto, SearchConfiguration::None) => {
                true
            }
            (
                WebSearchPolicy::Auto,
                SearchConfiguration::External | SearchConfiguration::Unsupported,
            ) => false,
            (WebSearchPolicy::Disabled, _) => unreachable!("disabled returns before inspection"),
        })
    }

    fn configure_catalogs(
        &self,
        harness: HarnessKind,
        models: &[CodingModelProfile],
        provider_base_url: &str,
        api_key: &str,
        search_managed: bool,
    ) -> Result<Option<IntegrationChange>, ConfigurationError> {
        let change = match harness {
            HarnessKind::OpenCode => Some(self.legacy.configure_opencode(
                models,
                provider_base_url,
                search_managed.then_some((api_key, provider_base_url)),
            )?),
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

fn receipt_manages_content(receipt: &DocumentReceipt) -> bool {
    match receipt {
        DocumentReceipt::Json(receipt) => !receipt.entries.is_empty(),
        DocumentReceipt::Yaml(receipt) => !receipt.entries.is_empty(),
        DocumentReceipt::TextBlock(receipt) => receipt.active,
        DocumentReceipt::ExactFile(receipt) => receipt.active,
        DocumentReceipt::Toml(_) => true,
    }
}

#[derive(Debug)]
pub(crate) struct ConfigurationChange {
    changed: bool,
    paths: Vec<PathBuf>,
    model_count: usize,
    search: ManagedSearchStatus,
}

#[derive(Debug, Clone, Copy)]
struct ManagedSearchStatus {
    policy: WebSearchPolicy,
    managed: bool,
}

fn pi_family_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: ManagedSearchStatus,
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

fn omp_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: ManagedSearchStatus,
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

fn preferred_yaml_path(directory: &Path, canonical: &str, compatible: &str) -> PathBuf {
    let canonical = directory.join(canonical);
    let compatible = directory.join(compatible);
    if !canonical.exists() && compatible.exists() {
        compatible
    } else {
        canonical
    }
}

fn cline_plans(
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

fn search_mcp_plan(path: PathBuf, api_key: &str, base_url: &str, enabled: bool) -> DocumentPlan {
    let entries = enabled
        .then(|| {
            exclusive_json(
                &["mcpServers", SEARCH_MCP_ID],
                json!({
                    "command": "nan-harness",
                    "args": [
                        "__search-mcp",
                        "--provider-base-url",
                        base_url,
                        "--token-env",
                        SEARCH_TOKEN_ENVIRONMENT
                    ],
                    "env": {"NAN_HARNESS_SEARCH_API_KEY": api_key},
                    "enabled": true
                }),
            )
        })
        .into_iter()
        .collect();
    DocumentPlan::Json(JsonPlan { path, entries })
}

fn qwen_plans(
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

fn deepseek_plans(
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

fn deepseek_search_plan(
    directory: &Path,
    base_url: &str,
    enabled: bool,
) -> Result<DocumentPlan, ConfigurationError> {
    let body = if enabled {
        Some(format!(
            "- insert:\n    - id: mcp-nan-search\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: nan-search\n        transport: stdio\n        command: nan-harness\n        args: ['__search-mcp', '--provider-base-url', {}, '--token-env', 'NAN_API_KEY']\n        env:\n          NAN_API_KEY: !!js process.env.NAN_API_KEY",
            yaml_quote(base_url)?
        ))
    } else {
        None
    };
    Ok(DocumentPlan::TextBlock(TextBlockPlan {
        path: directory.join("cordis.patch.yml"),
        begin: "# nan-harness:begin search-mcp".to_owned(),
        end: "# nan-harness:end search-mcp".to_owned(),
        body,
        conflicting_keys: vec!["- id: mcp-nan-search".to_owned()],
    }))
}

fn hermes_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    default_model: &str,
    search_managed: bool,
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
    ])
}

fn hermes_search_provider() -> String {
    r#"import os
from pathlib import Path

import httpx
import yaml

from agent.web_search_provider import WebSearchProvider


def _connection():
    home = Path(os.environ.get("HERMES_HOME", Path.home() / ".hermes"))
    with (home / "config.yaml").open(encoding="utf-8") as stream:
        model = (yaml.safe_load(stream) or {}).get("model", {})
    return str(model.get("api_key", "")).strip(), str(model.get("base_url", "")).rstrip("/")


class NanHarnessWebSearchProvider(WebSearchProvider):
    @property
    def name(self):
        return "nan-harness"

    @property
    def display_name(self):
        return "nan-search"

    def is_available(self):
        try:
            api_key, base_url = _connection()
            return bool(api_key and base_url)
        except Exception:
            return False

    def search(self, query, limit=5):
        try:
            api_key, base_url = _connection()
            response = httpx.post(
                f"{base_url}/search",
                headers={"Authorization": f"Bearer {api_key}"},
                json={"query": query, "maxResults": min(max(int(limit), 1), 20)},
                timeout=60,
            )
            response.raise_for_status()
            results = response.json().get("results", [])
            return {
                "success": True,
                "data": {
                    "web": [
                        {
                            "title": item.get("title", ""),
                            "url": item.get("url", ""),
                            "description": item.get("snippet", ""),
                            "position": position,
                        }
                        for position, item in enumerate(results, start=1)
                    ]
                },
            }
        except Exception:
            return {"success": False, "error": "NH-SEARCH-HTTP"}
"#
    .to_owned()
}

fn openclaw_plans(
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

fn openclaw_search_plugin() -> String {
    r#"import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";

const parameters = {
  type: "object",
  properties: {
    query: { type: "string" },
    count: { type: "integer", minimum: 1, maximum: 20 }
  },
  required: ["query"],
  additionalProperties: false
};

export default definePluginEntry({
  id: "nan-harness-search",
  name: "nan-search",
  description: "nan-search",
  register(api) {
    const connection = () => {
      const provider = api.config?.models?.providers?.nan ?? {};
      return {
        apiKey: typeof provider.apiKey === "string" ? provider.apiKey : "",
        baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl.replace(/\/+$/, "") : ""
      };
    };
    api.registerWebSearchProvider({
      id: "nan-harness",
      label: "nan-search",
      hint: "nan-search",
      requiresCredential: true,
      envVars: [],
      placeholder: "nan-session",
      signupUrl: "https://nan.im",
      credentialPath: "",
      getCredentialValue: () => connection().apiKey,
      setCredentialValue: () => {},
      createTool: () => ({
        description: "nan-search",
        parameters,
        execute: async (args, context) => {
          const query = typeof args.query === "string" ? args.query.trim() : "";
          if (!query) throw new Error("NH-SEARCH-QUERY");
          const count = Number.isInteger(args.count) ? Math.min(Math.max(args.count, 1), 20) : 5;
          const { apiKey, baseUrl } = connection();
          const response = await fetch(`${baseUrl}/search`, {
            method: "POST",
            headers: {
              authorization: `Bearer ${apiKey}`,
              "content-type": "application/json"
            },
            body: JSON.stringify({ query, maxResults: count }),
            signal: context?.signal
          });
          if (!response.ok) throw new Error(`NH-SEARCH-HTTP-${response.status}`);
          const payload = await response.json();
          const results = Array.isArray(payload.results) ? payload.results : [];
          return {
            query,
            provider: "nan-harness",
            count: results.length,
            externalContent: { untrusted: true, source: "web_search", provider: "nan-harness" },
            results
          };
        }
      })
    });
  }
});
"#
    .to_owned()
}

fn goose_plans(
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
            legacy_block: Some(LegacyTextBlock {
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
            }),
        }),
    ])
}

fn goose_config_entries(
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

fn to_yaml_value(value: Value) -> Result<YamlValue, ConfigurationError> {
    serde_yaml_ng::to_value(value).map_err(ConfigurationError::SerializeYaml)
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

fn omp_provider(api_key: &str, base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": api_key,
        "authHeader": true,
        "models": models.iter().map(omp_model).collect::<Vec<_>>()
    })
}

fn omp_model(model: &CodingModelProfile) -> Value {
    let mut value = pi_model(model);
    if let ReasoningPolicy::Effort { supported, default } = model.reasoning {
        let supported = supported
            .into_iter()
            .map(|effort| Value::String(reasoning_effort_name(effort).to_owned()))
            .collect::<Vec<_>>();
        let default = Value::String(reasoning_effort_name(default).to_owned());
        let effort_map = Value::Object(
            supported
                .iter()
                .filter_map(|effort| {
                    effort
                        .as_str()
                        .map(|name| (name.to_owned(), Value::String(name.to_owned())))
                })
                .collect(),
        );
        value["thinking"] = json!({
            "mode": "effort",
            "efforts": supported,
            "defaultLevel": default,
            "effortMap": effort_map.clone()
        });
        value["compat"]["reasoningEffortMap"] = effort_map;
    }
    value
}

const fn reasoning_effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
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

fn append_unique_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::AppendUnique,
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
