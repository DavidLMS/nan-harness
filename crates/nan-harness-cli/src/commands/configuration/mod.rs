mod catalog;
mod command;
mod documents;
mod error;
mod lifecycle;
mod paths;
mod plans;
mod state;

use catalog::{catalog_integration, legacy_harness};
pub(crate) use command::run;
use documents::{
    apply_prepared, document_is_active, dotenv_quote, prepare_documents, prepare_removals,
    rollback_prepared, sha256, yaml_quote,
};
pub(crate) use error::ConfigurationError;
pub(crate) use lifecycle::ConfigurationManager;
use paths::ConfigurationPaths;
use plans::{
    DocumentPlan, ExactFilePlan, JsonEntryMode, JsonPlan, KimiPlan, TextBlockPlan, YamlEntryMode,
    YamlPlan, ensure_supported, for_harness, preferred_model,
};
use state::{
    ConfigurationChange, ConfigurationState, DocumentReceipt, ExactFileReceipt, HarnessReceipt,
    JsonEntryReceipt, JsonReceipt, ManagedSearchStatus, PreparedDocument, TextBlockReceipt,
    TomlReceipt, YamlEntryReceipt, YamlReceipt, receipt_manages_content,
};

#[cfg(test)]
use plans::{
    LegacyTextBlock, YamlEntryPlan, exclusive_json, hermes_search_provider, openclaw_search_plugin,
    override_json, pi_family_plans, search_mcp_plan,
};

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

#[cfg(test)]
mod tests;
