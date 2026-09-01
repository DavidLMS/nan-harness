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

pub(crate) fn document_is_active(receipt: &DocumentReceipt) -> bool {
    match receipt {
        DocumentReceipt::Json(receipt) if receipt.entries.is_empty() => {
            !receipt.path.exists()
                || fs::read(&receipt.path)
                    .ok()
                    .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
                    .is_some_and(|document| document.is_object())
        }
        DocumentReceipt::Json(receipt) => fs::read(&receipt.path)
            .ok()
            .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
            .is_some_and(|document| {
                receipt.entries.iter().all(|entry| {
                    get_json_path(&document, &entry.path)
                        .and_then(|value| hash_json(value).ok())
                        .is_some_and(|hash| hash == entry.value_sha256)
                })
            }),
        DocumentReceipt::Yaml(receipt) if receipt.entries.is_empty() => {
            !receipt.path.exists()
                || fs::read(&receipt.path)
                    .ok()
                    .and_then(|contents| serde_yaml_ng::from_slice::<YamlValue>(&contents).ok())
                    .is_some_and(|document| document.is_mapping())
        }
        DocumentReceipt::Yaml(receipt) => fs::read(&receipt.path)
            .ok()
            .and_then(|contents| serde_yaml_ng::from_slice::<YamlValue>(&contents).ok())
            .is_some_and(|document| {
                receipt.entries.iter().all(|entry| {
                    get_yaml_path(&document, &entry.path)
                        .and_then(|value| hash_yaml(value).ok())
                        .is_some_and(|hash| hash == entry.value_sha256)
                })
            }),
        DocumentReceipt::TextBlock(receipt) if !receipt.active => true,
        DocumentReceipt::TextBlock(receipt) => fs::read_to_string(&receipt.path)
            .ok()
            .and_then(|source| {
                block_range(&source, &receipt.begin, &receipt.end)
                    .ok()
                    .flatten()
                    .map(|range| (source, range))
            })
            .is_some_and(|(source, range)| {
                sha256(source[range].as_bytes()) == receipt.block_sha256
            }),
        DocumentReceipt::ExactFile(receipt) => {
            !receipt.active
                || fs::read(&receipt.path).is_ok_and(|contents| sha256(&contents) == receipt.sha256)
        }
        DocumentReceipt::Toml(receipt) => fs::read_to_string(&receipt.path)
            .ok()
            .and_then(|source| source.parse::<DocumentMut>().ok())
            .is_some_and(|document| kimi_receipt_is_active(&document, receipt)),
    }
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
