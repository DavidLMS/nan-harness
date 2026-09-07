use super::*;

pub(crate) fn apply_prepared(documents: &[PreparedDocument]) -> Result<(), ConfigurationError> {
    for (index, document) in documents.iter().enumerate() {
        let result = match &document.replacement {
            Some(payload) => {
                write_private_file(&document.path, payload, None).map_err(ConfigurationError::from)
            }
            None => remove_optional_file(&document.path),
        };
        if let Err(error) = result {
            rollback_prepared(&documents[..index]);
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn rollback_prepared(documents: &[PreparedDocument]) {
    for document in documents.iter().rev() {
        match &document.original {
            Some(payload) => {
                let _ = write_private_file(&document.path, payload, document.permissions.as_ref());
            }
            None => {
                let _ = fs::remove_file(&document.path);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn document_is_active(receipt: &DocumentReceipt) -> bool {
    inspect_document(receipt).is_active()
}

pub(crate) fn inspect_document(receipt: &DocumentReceipt) -> ConfigurationHealth {
    inspect_document_result(receipt).unwrap_or_else(|health| health)
}

fn inspect_document_result(
    receipt: &DocumentReceipt,
) -> Result<ConfigurationHealth, ConfigurationHealth> {
    use ConfigurationHealth::{Active, Invalid, Missing};
    let (path, empty) = match receipt {
        DocumentReceipt::Json(receipt) => (&receipt.path, receipt.entries.is_empty()),
        DocumentReceipt::Yaml(receipt) => (&receipt.path, receipt.entries.is_empty()),
        DocumentReceipt::TextBlock(receipt) if !receipt.active => return Ok(Active),
        DocumentReceipt::ExactFile(receipt) if !receipt.active => return Ok(Active),
        DocumentReceipt::TextBlock(receipt) => (&receipt.path, false),
        DocumentReceipt::ExactFile(receipt) => (&receipt.path, false),
        DocumentReceipt::Toml(receipt) => (&receipt.path, false),
    };
    let contents = match read_managed_document(path) {
        Err(Missing) if empty => return Ok(Active),
        result => result?,
    };
    let matches = match receipt {
        DocumentReceipt::Json(receipt) => {
            let document: Value = serde_json::from_slice(&contents).map_err(|_| Invalid)?;
            if !document.is_object() {
                return Err(Invalid);
            }
            receipt.entries.iter().all(|entry| {
                get_json_path(&document, &entry.path)
                    .and_then(|value| hash_json(value).ok())
                    .is_some_and(|hash| hash == entry.value_sha256)
            })
        }
        DocumentReceipt::Yaml(receipt) => {
            let document: YamlValue = serde_yaml_ng::from_slice(&contents).map_err(|_| Invalid)?;
            if !document.is_mapping() {
                return Err(Invalid);
            }
            receipt.entries.iter().all(|entry| {
                get_yaml_path(&document, &entry.path)
                    .and_then(|value| hash_yaml(value).ok())
                    .is_some_and(|hash| hash == entry.value_sha256)
            })
        }
        DocumentReceipt::TextBlock(receipt) => {
            let source = std::str::from_utf8(&contents).map_err(|_| Invalid)?;
            block_range(source, &receipt.begin, &receipt.end)
                .map_err(|_| Invalid)?
                .is_some_and(|range| sha256(source[range].as_bytes()) == receipt.block_sha256)
        }
        DocumentReceipt::ExactFile(receipt) => sha256(&contents) == receipt.sha256,
        DocumentReceipt::Toml(receipt) => {
            let source = std::str::from_utf8(&contents).map_err(|_| Invalid)?;
            let document = source.parse::<DocumentMut>().map_err(|_| Invalid)?;
            kimi_receipt_is_active(&document, receipt)
        }
    };
    Ok(ConfigurationHealth::from_matches(matches))
}

pub(crate) fn kimi_receipt_is_active(document: &DocumentMut, receipt: &TomlReceipt) -> bool {
    document
        .get("providers")
        .and_then(Item::as_table_like)
        .and_then(|providers| providers.get("nan"))
        .and_then(|provider| hash_toml_item(provider).ok())
        .is_some_and(|hash| hash == receipt.provider_sha256)
        && document
            .get("default_model")
            .and_then(|item| hash_toml_item(item).ok())
            .is_some_and(|hash| hash == receipt.default_model_sha256)
        && receipt.models.iter().all(|(name, expected_hash)| {
            document
                .get("models")
                .and_then(Item::as_table_like)
                .and_then(|models| models.get(name))
                .and_then(|item| hash_toml_item(item).ok())
                .is_some_and(|hash| hash == *expected_hash)
        })
}

pub(crate) fn table_mut_or_create<'a>(
    document: &'a mut DocumentMut,
    name: &str,
    path: &Path,
) -> Result<&'a mut Table, ConfigurationError> {
    if !document.as_table().contains_key(name) {
        document[name] = Item::Table(Table::new());
    }
    document[name]
        .as_table_mut()
        .ok_or_else(|| ConfigurationError::TomlFieldNotTable {
            path: path.to_path_buf(),
            field: name.to_owned(),
        })
}

pub(crate) fn remove_toml_child(
    document: &mut DocumentMut,
    parent: &str,
    child: &str,
    path: &Path,
) -> Result<(), ConfigurationError> {
    document[parent]
        .as_table_mut()
        .ok_or_else(|| ConfigurationError::TomlFieldNotTable {
            path: path.to_path_buf(),
            field: parent.to_owned(),
        })?
        .remove(child);
    Ok(())
}

pub(crate) fn remove_empty_toml_table(document: &mut DocumentMut, name: &str) {
    if document[name].as_table().is_some_and(Table::is_empty) {
        document.remove(name);
    }
}

pub(crate) fn hash_toml_item(item: &Item) -> Result<String, ConfigurationError> {
    let mut wrapper = DocumentMut::new();
    wrapper["value"] = item.clone();
    let semantic = toml_edit::de::from_document::<Value>(wrapper)
        .map_err(ConfigurationError::NormalizeToml)?;
    hash_json(&semantic["value"])
}

pub(crate) fn toml_integer(
    value: u64,
    field: &'static str,
    model: &str,
) -> Result<i64, ConfigurationError> {
    i64::try_from(value).map_err(|_| ConfigurationError::ModelValueOutOfRange {
        field,
        model: model.to_owned(),
    })
}

pub(crate) fn kimi_model_name(model_id: &str) -> String {
    format!("nan/{model_id}")
}
