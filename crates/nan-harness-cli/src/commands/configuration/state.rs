use super::STATE_SCHEMA_VERSION;
use nan_harness_core::WebSearchPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml_ng::Value as YamlValue;
use std::collections::BTreeMap;
use std::fs::Permissions;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigurationState {
    pub(crate) schema_version: u8,
    #[serde(default)]
    pub(crate) harnesses: BTreeMap<String, HarnessReceipt>,
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
pub(crate) struct HarnessReceipt {
    pub(crate) credential_fingerprint: String,
    pub(crate) model_ids: Vec<String>,
    #[serde(default)]
    pub(crate) search_policy: WebSearchPolicy,
    #[serde(default)]
    pub(crate) search_managed: bool,
    pub(crate) documents: Vec<DocumentReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "format", rename_all = "kebab-case")]
pub(crate) enum DocumentReceipt {
    Json(JsonReceipt),
    Yaml(YamlReceipt),
    TextBlock(TextBlockReceipt),
    ExactFile(ExactFileReceipt),
    Toml(TomlReceipt),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct YamlReceipt {
    pub(crate) path: PathBuf,
    pub(crate) created_file: bool,
    pub(crate) entries: Vec<YamlEntryReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct YamlEntryReceipt {
    pub(crate) path: Vec<String>,
    pub(crate) value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous: Option<YamlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JsonReceipt {
    pub(crate) path: PathBuf,
    pub(crate) created_file: bool,
    pub(crate) entries: Vec<JsonEntryReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct JsonEntryReceipt {
    pub(crate) path: Vec<String>,
    pub(crate) value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TextBlockReceipt {
    pub(crate) path: PathBuf,
    pub(crate) created_file: bool,
    pub(crate) begin: String,
    pub(crate) end: String,
    pub(crate) block_sha256: String,
    #[serde(default = "default_true")]
    pub(crate) active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExactFileReceipt {
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
    #[serde(default = "default_true")]
    pub(crate) active: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TomlReceipt {
    pub(crate) path: PathBuf,
    pub(crate) created_file: bool,
    pub(crate) provider_sha256: String,
    pub(crate) models: BTreeMap<String, String>,
    pub(crate) default_model_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_default_model: Option<String>,
}

pub(crate) struct PreparedDocument {
    pub(crate) path: PathBuf,
    pub(crate) original: Option<Vec<u8>>,
    pub(crate) permissions: Option<Permissions>,
    pub(crate) replacement: Option<Vec<u8>>,
    pub(crate) receipt: DocumentReceipt,
}

pub(crate) fn receipt_manages_content(receipt: &DocumentReceipt) -> bool {
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
    pub(crate) changed: bool,
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) model_count: usize,
    pub(crate) search: ManagedSearchStatus,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ManagedSearchStatus {
    pub(crate) policy: WebSearchPolicy,
    pub(crate) managed: bool,
}
