use super::ZedDesktopError;
use super::paths::SessionReceipt;
use crate::commands::desktop::reject_symlink;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use nan_harness_core::{CodingModelProfile, ReasoningEffort, ReasoningPolicy};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const PROVIDER_ID: &str = "nan";
const BACKUP_FILE: &str = "settings.backup";

#[derive(Debug)]
pub(super) struct PatchedSettings {
    pub(super) contents: Vec<u8>,
    pub(super) provider_sha256: String,
    pub(super) default_model_sha256: String,
    pub(super) created_language_models: bool,
    pub(super) created_openai_compatible: bool,
    pub(super) created_agent: bool,
    pub(super) previous_default_model: Option<Value>,
}

pub(super) fn patch_settings(
    original: Option<&[u8]>,
    gateway_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
) -> Result<PatchedSettings, ZedDesktopError> {
    let source = match original {
        Some(contents) => std::str::from_utf8(contents).map_err(ZedDesktopError::SettingsUtf8)?,
        None => "{}\n",
    };
    let root = parse_jsonc(source)?;
    let root_object = root
        .object_value()
        .ok_or(ZedDesktopError::SettingsRootNotObject)?;

    let language_models_property = root_object.get("language_models");
    let created_language_models = language_models_property.is_none();
    let language_models = match language_models_property {
        Some(property) => property
            .object_value()
            .ok_or(ZedDesktopError::SettingsFieldNotObject("language_models"))?,
        None => root_object.object_value_or_set("language_models"),
    };
    let compatible_property = language_models.get("openai_compatible");
    let created_openai_compatible = compatible_property.is_none();
    let compatible = match compatible_property {
        Some(property) => {
            property
                .object_value()
                .ok_or(ZedDesktopError::SettingsFieldNotObject(
                    "language_models.openai_compatible",
                ))?
        }
        None => language_models.object_value_or_set("openai_compatible"),
    };
    if compatible.get(PROVIDER_ID).is_some() {
        return Err(ZedDesktopError::UnmanagedProviderConflict);
    }

    let provider = zed_provider(gateway_url, models);
    let provider_sha256 = hash_input_value(&provider)?;
    compatible.append(PROVIDER_ID, provider);

    let agent_property = root_object.get("agent");
    let created_agent = agent_property.is_none();
    let agent = match agent_property {
        Some(property) => property
            .object_value()
            .ok_or(ZedDesktopError::SettingsFieldNotObject("agent"))?,
        None => root_object.object_value_or_set("agent"),
    };
    let default_model = zed_default_model(selected_model);
    let default_model_sha256 = hash_input_value(&default_model)?;
    let previous_default_model = if let Some(property) = agent.get("default_model") {
        let previous = property
            .to_serde_value()
            .ok_or(ZedDesktopError::InvalidDefaultModel)?;
        if !previous.is_object() {
            return Err(ZedDesktopError::InvalidDefaultModel);
        }
        property.set_value(default_model);
        Some(previous)
    } else {
        agent.append("default_model", default_model);
        None
    };

    Ok(PatchedSettings {
        contents: root.to_string().into_bytes(),
        provider_sha256,
        default_model_sha256,
        created_language_models,
        created_openai_compatible,
        created_agent,
        previous_default_model,
    })
}

pub(super) fn remove_managed_settings(
    current: &[u8],
    receipt: &SessionReceipt,
) -> Result<Option<Vec<u8>>, ZedDesktopError> {
    let source = std::str::from_utf8(current).map_err(ZedDesktopError::SettingsUtf8)?;
    let root = parse_jsonc(source)?;
    let root_object = root
        .object_value()
        .ok_or(ZedDesktopError::SettingsRootNotObject)?;

    let language_models = root_object
        .object_value("language_models")
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    let compatible = language_models
        .object_value("openai_compatible")
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    let provider = compatible
        .get(PROVIDER_ID)
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    let provider_value = provider
        .to_serde_value()
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    if hash_json_value(&provider_value)? != receipt.applied_provider_sha256 {
        return Err(ZedDesktopError::ManagedConfigurationChanged);
    }

    let agent = root_object
        .object_value("agent")
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    let default_model = agent
        .get("default_model")
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    let default_model_value = default_model
        .to_serde_value()
        .ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    if hash_json_value(&default_model_value)? != receipt.applied_default_model_sha256 {
        return Err(ZedDesktopError::ManagedConfigurationChanged);
    }

    provider.remove();
    if receipt.created_openai_compatible && compatible.properties().is_empty() {
        language_models
            .get("openai_compatible")
            .ok_or(ZedDesktopError::ManagedConfigurationChanged)?
            .remove();
    }
    if receipt.created_language_models && language_models.properties().is_empty() {
        root_object
            .get("language_models")
            .ok_or(ZedDesktopError::ManagedConfigurationChanged)?
            .remove();
    }

    match &receipt.previous_default_model {
        Some(previous) => default_model.set_value(serde_to_input(previous)?),
        None => default_model.remove(),
    }
    if receipt.created_agent && agent.properties().is_empty() {
        root_object
            .get("agent")
            .ok_or(ZedDesktopError::ManagedConfigurationChanged)?
            .remove();
    }

    let rendered = root.to_string();
    if !receipt.file_existed
        && root_object.properties().is_empty()
        && empty_jsonc_object_is_disposable(&rendered)
    {
        Ok(None)
    } else {
        Ok(Some(rendered.into_bytes()))
    }
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ZedDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ZedDesktopError::ReadSettings(error)),
    }
}

pub(super) fn sha256(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

pub(super) fn backup_file_name() -> &'static str {
    BACKUP_FILE
}

fn zed_provider(gateway_url: &str, models: &[CodingModelProfile]) -> CstInputValue {
    CstInputValue::Object(vec![
        (
            "api_url".to_owned(),
            CstInputValue::String(gateway_url.to_owned()),
        ),
        (
            "available_models".to_owned(),
            CstInputValue::Array(models.iter().map(zed_model).collect()),
        ),
    ])
}

fn zed_model(model: &CodingModelProfile) -> CstInputValue {
    let mut fields = vec![
        ("name".to_owned(), CstInputValue::String(model.id.clone())),
        (
            "display_name".to_owned(),
            CstInputValue::String(model.display_name.clone()),
        ),
        (
            "max_tokens".to_owned(),
            CstInputValue::Number(model.context_window.to_string()),
        ),
        (
            "max_output_tokens".to_owned(),
            CstInputValue::Number(model.max_output_tokens.to_string()),
        ),
    ];
    if let ReasoningPolicy::Effort { default, .. } = model.reasoning {
        fields.push((
            "reasoning_effort".to_owned(),
            CstInputValue::String(reasoning_effort(default).to_owned()),
        ));
    }
    fields.push((
        "capabilities".to_owned(),
        CstInputValue::Object(vec![
            ("tools".to_owned(), CstInputValue::Bool(true)),
            ("images".to_owned(), CstInputValue::Bool(model.image_input)),
            ("parallel_tool_calls".to_owned(), CstInputValue::Bool(false)),
            ("prompt_cache_key".to_owned(), CstInputValue::Bool(false)),
            ("chat_completions".to_owned(), CstInputValue::Bool(true)),
            (
                "interleaved_reasoning".to_owned(),
                CstInputValue::Bool(false),
            ),
            ("max_tokens_parameter".to_owned(), CstInputValue::Bool(true)),
        ]),
    ));
    CstInputValue::Object(fields)
}

fn zed_default_model(selected_model: &str) -> CstInputValue {
    CstInputValue::Object(vec![
        (
            "provider".to_owned(),
            CstInputValue::String(PROVIDER_ID.to_owned()),
        ),
        (
            "model".to_owned(),
            CstInputValue::String(selected_model.to_owned()),
        ),
    ])
}

const fn reasoning_effort(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
}

fn parse_jsonc(source: &str) -> Result<CstRootNode, ZedDesktopError> {
    CstRootNode::parse(source, &ParseOptions::default())
        .map_err(|error| ZedDesktopError::ParseSettings(error.to_string()))
}

fn hash_input_value(value: &CstInputValue) -> Result<String, ZedDesktopError> {
    let root = CstRootNode::parse("{}", &ParseOptions::default())
        .map_err(|error| ZedDesktopError::GenerateSettings(error.to_string()))?;
    root.set_value(value.clone());
    let value = root
        .to_serde_value()
        .ok_or_else(|| ZedDesktopError::GenerateSettings("empty generated value".to_owned()))?;
    hash_json_value(&value)
}

fn hash_json_value(value: &Value) -> Result<String, ZedDesktopError> {
    serde_json::to_vec(value)
        .map(|encoded| sha256(&encoded))
        .map_err(ZedDesktopError::Serialize)
}

fn serde_to_input(value: &Value) -> Result<CstInputValue, ZedDesktopError> {
    match value {
        Value::Null => Ok(CstInputValue::Null),
        Value::Bool(value) => Ok(CstInputValue::Bool(*value)),
        Value::Number(value) => Ok(CstInputValue::Number(value.to_string())),
        Value::String(value) => Ok(CstInputValue::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(serde_to_input)
            .collect::<Result<Vec<_>, _>>()
            .map(CstInputValue::Array),
        Value::Object(values) => values
            .iter()
            .map(|(name, value)| Ok((name.clone(), serde_to_input(value)?)))
            .collect::<Result<Vec<_>, _>>()
            .map(CstInputValue::Object),
    }
}

fn empty_jsonc_object_is_disposable(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '{' | '}'))
}
