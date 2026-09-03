use super::PersistenceError;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use nan_harness_core::CodingModelProfile;
use sha2::{Digest as _, Sha256};
use std::fs::Permissions;
use std::path::Path;
use std::path::PathBuf;

pub(super) const PI_EXTENSION_RELATIVE_PATH: &str = ".pi/agent/extensions/nan-provider.js";
pub(super) const LEGACY_PI_EXTENSION_RELATIVE_PATH: &str = ".pi/agent/extensions/nan-provider.mjs";
pub(super) const PRIME_EXTENSION_RELATIVE_PATH: &str = ".prime/agent/extensions/nan-provider.js";
pub(super) const AIDER_SETTINGS_RELATIVE_PATH: &str = ".aider.model.settings.yml";
pub(super) const AIDER_METADATA_RELATIVE_PATH: &str = ".aider.model.metadata.json";
pub(super) const DEEPSEEK_BLOCK_BEGIN: &str = "# nan-harness:begin deepseek-provider";
pub(super) const DEEPSEEK_BLOCK_END: &str = "# nan-harness:end deepseek-provider";
pub(super) const AIDER_BLOCK_BEGIN: &str = "# nan-harness:begin aider-models";
pub(super) const AIDER_BLOCK_END: &str = "# nan-harness:end aider-models";
pub(super) const OPENCODE_CONFIG_DIRECTORY: &str = ".config/opencode";
pub(super) const OPENCODE_JSON: &str = "opencode.json";
pub(super) const OPENCODE_JSONC: &str = "opencode.jsonc";

#[derive(Debug, Clone, Copy)]
pub(super) struct ManagedBlockFormat<'a> {
    pub(super) begin: &'a str,
    pub(super) end: &'a str,
    pub(super) conflicting_keys: &'a [&'a str],
}

pub(super) struct PreparedFileChange {
    pub(super) path: PathBuf,
    pub(super) original: Option<Vec<u8>>,
    pub(super) original_permissions: Option<Permissions>,
    pub(super) replacement: Option<Vec<u8>>,
}

pub(super) fn opencode_provider(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> CstInputValue {
    let models = models
        .iter()
        .map(|model| {
            (
                model.id.clone(),
                CstInputValue::Object(vec![
                    (
                        "name".to_owned(),
                        CstInputValue::String(model.display_name.clone()),
                    ),
                    (
                        "description".to_owned(),
                        CstInputValue::String(model.description.clone()),
                    ),
                    (
                        "limit".to_owned(),
                        CstInputValue::Object(vec![
                            (
                                "context".to_owned(),
                                CstInputValue::Number(model.context_window.to_string()),
                            ),
                            (
                                "output".to_owned(),
                                CstInputValue::Number(model.max_output_tokens.to_string()),
                            ),
                        ]),
                    ),
                ]),
            )
        })
        .collect();
    CstInputValue::Object(vec![
        (
            "npm".to_owned(),
            CstInputValue::String("@ai-sdk/openai-compatible".to_owned()),
        ),
        ("name".to_owned(), CstInputValue::String("NaN".to_owned())),
        (
            "options".to_owned(),
            CstInputValue::Object(vec![(
                "baseURL".to_owned(),
                CstInputValue::String(provider_base_url.to_owned()),
            )]),
        ),
        ("models".to_owned(), CstInputValue::Object(models)),
    ])
}

pub(super) fn parse_jsonc(source: &str, path: &Path) -> Result<CstRootNode, PersistenceError> {
    CstRootNode::parse(source, &ParseOptions::default()).map_err(|error| {
        PersistenceError::ParseOpenCodeConfig {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

pub(super) fn parse_named_jsonc(
    source: &str,
    path: &Path,
    harness: &'static str,
) -> Result<CstRootNode, PersistenceError> {
    CstRootNode::parse(source, &ParseOptions::default()).map_err(|error| {
        PersistenceError::ParseHarnessConfig {
            harness,
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

pub(super) fn hash_input_value(value: &CstInputValue) -> Result<String, PersistenceError> {
    let root = CstRootNode::parse("{}", &ParseOptions::default())
        .map_err(|error| PersistenceError::GenerateOpenCodeProvider(error.to_string()))?;
    root.set_value(value.clone());
    let value = root
        .to_serde_value()
        .ok_or(PersistenceError::GenerateOpenCodeProvider(
            "provider value is empty".to_owned(),
        ))?;
    hash_json_value(&value)
}

pub(super) fn hash_json_value(value: &serde_json::Value) -> Result<String, PersistenceError> {
    let encoded = serde_json::to_vec(value).map_err(PersistenceError::SerializeProvider)?;
    Ok(sha256(&encoded))
}

pub(super) fn sha256(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn empty_jsonc_object_is_disposable(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '{' | '}'))
}

pub(super) fn validate_opencode_file_name(value: &str) -> Result<(), PersistenceError> {
    if matches!(value, OPENCODE_JSON | OPENCODE_JSONC) {
        Ok(())
    } else {
        Err(PersistenceError::InvalidReceiptPath(value.to_owned()))
    }
}
