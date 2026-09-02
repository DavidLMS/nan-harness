use super::PenDesktopError;
use crate::commands::desktop::{reject_symlink, write_private_atomic};
use nan_harness_core::{CodingModelProfile, ReasoningPolicy};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const PROVIDER_ID: &str = "nan";
const PROVIDER_NAME: &str = "NaN";

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PenDocumentKind {
    Models,
    Auth,
}

pub(super) fn patched_models_document(
    mut root: Map<String, Value>,
    base_url: &str,
    models: &[CodingModelProfile],
) -> Result<Vec<u8>, PenDesktopError> {
    let providers = object_field_mut(&mut root, "providers", PenDocumentKind::Models)?;
    providers.insert(
        PROVIDER_ID.to_owned(),
        json!({
            "name": PROVIDER_NAME,
            "baseUrl": base_url,
            "api": "openai-completions",
            "models": models.iter().map(pen_model).collect::<Vec<_>>()
        }),
    );
    serialize_document(&root)
}

pub(super) fn patched_auth_document(
    mut root: Map<String, Value>,
    api_key: &str,
) -> Result<Vec<u8>, PenDesktopError> {
    root.insert(
        PROVIDER_ID.to_owned(),
        json!({"type": "api_key", "key": api_key}),
    );
    serialize_document(&root)
}

fn pen_model(model: &CodingModelProfile) -> Value {
    let mut input = vec!["text"];
    if model.image_input {
        input.push("image");
    }
    json!({
        "id": model.id,
        "name": model.display_name,
        "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
        "input": input,
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window,
        "maxTokens": model.max_output_tokens
    })
}

pub(super) fn object_field_mut<'a>(
    root: &'a mut Map<String, Value>,
    field: &'static str,
    document: PenDocumentKind,
) -> Result<&'a mut Map<String, Value>, PenDesktopError> {
    let value = root
        .entry(field.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    value
        .as_object_mut()
        .ok_or(PenDesktopError::FieldNotObject { document, field })
}

pub(super) fn serialize_document(root: &Map<String, Value>) -> Result<Vec<u8>, PenDesktopError> {
    let mut bytes = serde_json::to_vec_pretty(root).map_err(PenDesktopError::Serialize)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn read_json_object(path: &Path) -> Result<Map<String, Value>, PenDesktopError> {
    let Some(contents) = read_optional(path)? else {
        return Ok(Map::new());
    };
    serde_json::from_slice::<Value>(&contents)
        .map_err(|source| PenDesktopError::ParseDocument {
            path: path.to_path_buf(),
            source,
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| PenDesktopError::DocumentRootNotObject(path.to_path_buf()))
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PenDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PenDesktopError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn provider_entry(
    contents: &[u8],
    kind: PenDocumentKind,
) -> Result<Value, PenDesktopError> {
    let root: Value = serde_json::from_slice(contents).map_err(|source| {
        PenDesktopError::ParseManagedDocument {
            document: kind,
            source,
        }
    })?;
    match kind {
        PenDocumentKind::Models => root
            .get("providers")
            .and_then(|providers| providers.get(PROVIDER_ID)),
        PenDocumentKind::Auth => root.get(PROVIDER_ID),
    }
    .cloned()
    .ok_or(PenDesktopError::ManagedEntryMissing(kind))
}

pub(super) fn merge_original_entry(
    current: &[u8],
    original: Option<&[u8]>,
    kind: PenDocumentKind,
) -> Result<Vec<u8>, PenDesktopError> {
    let mut current: Map<String, Value> = serde_json::from_slice::<Value>(current)
        .map_err(|source| PenDesktopError::ParseManagedDocument {
            document: kind,
            source,
        })?
        .as_object()
        .cloned()
        .ok_or(PenDesktopError::ManagedRootNotObject(kind))?;
    let previous = original
        .map(|contents| {
            serde_json::from_slice::<Value>(contents)
                .map_err(|source| PenDesktopError::ParseManagedDocument {
                    document: kind,
                    source,
                })
                .and_then(|value| {
                    value
                        .as_object()
                        .cloned()
                        .ok_or(PenDesktopError::ManagedRootNotObject(kind))
                })
        })
        .transpose()?;
    match kind {
        PenDocumentKind::Models => {
            let providers = object_field_mut(&mut current, "providers", kind)?;
            match previous
                .as_ref()
                .and_then(|root| root.get("providers"))
                .and_then(|providers| providers.get(PROVIDER_ID))
            {
                Some(value) => {
                    providers.insert(PROVIDER_ID.to_owned(), value.clone());
                }
                None => {
                    providers.remove(PROVIDER_ID);
                }
            }
        }
        PenDocumentKind::Auth => match previous.as_ref().and_then(|root| root.get(PROVIDER_ID)) {
            Some(value) => {
                current.insert(PROVIDER_ID.to_owned(), value.clone());
            }
            None => {
                current.remove(PROVIDER_ID);
            }
        },
    }
    serialize_document(&current)
}

pub(super) fn hash_value(value: &Value) -> Result<String, PenDesktopError> {
    serde_json::to_vec(value)
        .map(|contents| sha256(&contents))
        .map_err(PenDesktopError::Serialize)
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

pub(super) fn write_json_private(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), PenDesktopError> {
    let mut payload = serde_json::to_vec_pretty(value).map_err(PenDesktopError::Serialize)?;
    payload.push(b'\n');
    write_private_atomic(path, &payload)?;
    Ok(())
}
