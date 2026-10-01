use super::*;
use crate::commands::persistence::parse_named_jsonc;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};

pub(crate) fn parse_json_document(
    contents: &[u8],
    path: &Path,
    comments: bool,
) -> Result<Value, ConfigurationError> {
    if !comments {
        return serde_json::from_slice(contents).map_err(|source| {
            ConfigurationError::ParseDocument {
                path: path.to_path_buf(),
                source,
            }
        });
    }
    jsonc_root(contents, path)?
        .to_serde_value()
        .ok_or_else(|| ConfigurationError::DocumentRootNotObject(path.to_path_buf()))
}

pub(crate) fn json_replacement(
    document: &Value,
    original: Option<&[u8]>,
    path: &Path,
    comments: bool,
    created_file: bool,
) -> Result<Option<Vec<u8>>, ConfigurationError> {
    let rendered = render_json_document(document, original, path, comments)?;
    let disposable = !comments
        || rendered
            .iter()
            .all(|byte| byte.is_ascii_whitespace() || matches!(byte, b'{' | b'}'));
    if created_file && document.as_object().is_some_and(Map::is_empty) && disposable {
        Ok(None)
    } else {
        Ok(Some(rendered))
    }
}

pub(crate) fn render_json_document(
    document: &Value,
    original: Option<&[u8]>,
    path: &Path,
    comments: bool,
) -> Result<Vec<u8>, ConfigurationError> {
    if !comments {
        return serde_json::to_vec_pretty(document).map_err(ConfigurationError::SerializeDocument);
    }
    let root = jsonc_root(original.unwrap_or(b"{}\n"), path)?;
    let object = root
        .object_value()
        .ok_or_else(|| ConfigurationError::DocumentRootNotObject(path.to_path_buf()))?;
    let desired = document
        .as_object()
        .ok_or_else(|| ConfigurationError::DocumentRootNotObject(path.to_path_buf()))?;
    update_jsonc_object(&object, desired)?;
    Ok(root.to_string().into_bytes())
}

fn jsonc_root(contents: &[u8], path: &Path) -> Result<CstRootNode, ConfigurationError> {
    let source =
        std::str::from_utf8(contents).map_err(|error| ConfigurationError::ReadDocument {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        })?;
    let root = parse_named_jsonc(source, path, "MiMo Code")?;
    if let Some(object) = root.object_value() {
        validate_jsonc_keys(&object, path)?;
    }
    Ok(root)
}

fn update_jsonc_object(
    object: &CstObject,
    desired: &Map<String, Value>,
) -> Result<(), ConfigurationError> {
    for property in object.properties() {
        let name = property
            .name()
            .and_then(|name| name.decoded_value().ok())
            .ok_or(ConfigurationError::InvalidManagedPath)?;
        if !desired.contains_key(&name) {
            property.remove();
        }
    }
    for (name, value) in desired {
        let Some(property) = object.get(name) else {
            object.append(name, jsonc_input(value));
            continue;
        };
        if property.to_serde_value().as_ref() == Some(value) {
            continue;
        }
        // Recurse into existing objects so unrelated siblings retain their comments.
        if let (Some(child), Some(desired)) = (property.object_value(), value.as_object()) {
            update_jsonc_object(&child, desired)?;
        } else {
            property.set_value(jsonc_input(value));
        }
    }
    Ok(())
}

fn jsonc_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(values.iter().map(jsonc_input).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(name, value)| (name.clone(), jsonc_input(value)))
                .collect(),
        ),
    }
}

fn validate_jsonc_keys(object: &CstObject, path: &Path) -> Result<(), ConfigurationError> {
    let mut names = BTreeSet::new();
    for property in object.properties() {
        let name = property
            .name()
            .and_then(|name| name.decoded_value().ok())
            .ok_or(ConfigurationError::InvalidManagedPath)?;
        if !names.insert(name) {
            return Err(PersistenceError::ParseHarnessConfig {
                harness: "MiMo Code",
                path: path.to_path_buf(),
                message: "duplicate object keys are unsupported".to_owned(),
            }
            .into());
        }
        if let Some(child) = property.object_value() {
            validate_jsonc_keys(&child, path)?;
        }
    }
    Ok(())
}
