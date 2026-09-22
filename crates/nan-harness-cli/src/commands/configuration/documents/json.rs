use super::*;

pub(crate) fn prepare_json(
    plan: &JsonPlan,
    previous: Option<&JsonReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| receipt.path != plan.path) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let mut document = match original.as_deref() {
        Some(contents) => serde_json::from_slice::<Value>(contents).map_err(|source| {
            ConfigurationError::ParseDocument {
                path: plan.path.clone(),
                source,
            }
        })?,
        None => Value::Object(Map::new()),
    };
    if !document.is_object() {
        return Err(ConfigurationError::DocumentRootNotObject(plan.path.clone()));
    }
    let entries = prepare_json_entries(&mut document, plan, previous)?;
    let created_file = previous.map_or(original.is_none(), |receipt| receipt.created_file);
    let replacement = if entries.is_empty()
        && previous.is_none_or(|receipt| receipt.entries.is_empty())
    {
        original.clone()
    } else if created_file && document.as_object().is_some_and(Map::is_empty) {
        None
    } else {
        Some(serde_json::to_vec_pretty(&document).map_err(ConfigurationError::SerializeDocument)?)
    };
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Json(JsonReceipt {
            path: plan.path.clone(),
            created_file,
            entries,
        }),
    })
}

pub(crate) fn prepare_json_entries(
    document: &mut Value,
    plan: &JsonPlan,
    previous: Option<&JsonReceipt>,
) -> Result<Vec<JsonEntryReceipt>, ConfigurationError> {
    let previous_entries = merged_json_receipts(previous.map_or(&[], |receipt| &receipt.entries));
    for prior in previous_entries.values() {
        let current = get_json_path(document, &prior.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(plan.path.clone()))?;
        if hash_json(current)? != prior.value_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
    }
    let desired_paths = plan
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    for prior in previous_entries
        .values()
        .filter(|entry| !desired_paths.contains(&entry.path))
        .rev()
    {
        if let Some(value) = &prior.previous {
            set_json_path(document, &prior.path, value.clone(), &plan.path)?;
        } else {
            remove_json_path(document, &prior.path);
        }
    }
    let mut entries: Vec<JsonEntryReceipt> = Vec::with_capacity(plan.entries.len());
    for planned in &plan.entries {
        let prior = previous_entries.get(&planned.path);
        let current = get_json_path(document, &planned.path).cloned();
        if prior.is_none() && matches!(planned.mode, JsonEntryMode::Exclusive) && current.is_some()
        {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        let previous_value = match prior {
            Some(entry) => entry.previous.clone(),
            None => matches!(
                planned.mode,
                JsonEntryMode::Override | JsonEntryMode::AppendUnique
            )
            .then_some(current.clone())
            .flatten(),
        };
        let already_appended = entries.iter().any(|entry| entry.path == planned.path);
        let append_base = if prior.is_some() && !already_appended {
            previous_value.as_ref()
        } else {
            current.as_ref()
        };
        let desired = match planned.mode {
            JsonEntryMode::AppendUnique => append_unique_json_value(
                append_base,
                &planned.value,
                &plan.path,
                planned.path.last().map_or("", String::as_str),
            )?,
            JsonEntryMode::Exclusive | JsonEntryMode::Override => planned.value.clone(),
        };
        set_json_path(document, &planned.path, desired.clone(), &plan.path)?;
        if let Some(entry) = entries.iter_mut().find(|entry| entry.path == planned.path) {
            entry.value_sha256 = hash_json(&desired)?;
        } else {
            entries.push(JsonEntryReceipt {
                path: planned.path.clone(),
                value_sha256: hash_json(&desired)?,
                previous: previous_value,
            });
        }
    }
    Ok(entries)
}

pub(crate) fn prepare_json_removal(
    receipt: &JsonReceipt,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&receipt.path)?;
    let permissions = file_permissions(&receipt.path)?;
    let Some(contents) = original.as_deref() else {
        if receipt.entries.is_empty() {
            return Ok(PreparedDocument {
                path: receipt.path.clone(),
                original,
                permissions,
                replacement: None,
                receipt: DocumentReceipt::Json(receipt.clone()),
            });
        }
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    };
    let mut document = serde_json::from_slice::<Value>(contents).map_err(|source| {
        ConfigurationError::ParseDocument {
            path: receipt.path.clone(),
            source,
        }
    })?;
    let entries = merged_json_receipts(&receipt.entries);
    for entry in entries.values() {
        let current = get_json_path(&document, &entry.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
        if hash_json(current)? != entry.value_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                receipt.path.clone(),
            ));
        }
    }
    for entry in entries.values().rev() {
        if let Some(previous) = &entry.previous {
            set_json_path(&mut document, &entry.path, previous.clone(), &receipt.path)?;
        } else {
            remove_json_path(&mut document, &entry.path);
        }
    }
    let replacement = if receipt.created_file && document.as_object().is_some_and(Map::is_empty) {
        None
    } else {
        Some(serde_json::to_vec_pretty(&document).map_err(ConfigurationError::SerializeDocument)?)
    };
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Json(receipt.clone()),
    })
}

fn merged_json_receipts(entries: &[JsonEntryReceipt]) -> BTreeMap<Vec<String>, JsonEntryReceipt> {
    let mut previous_entries: BTreeMap<Vec<String>, JsonEntryReceipt> = BTreeMap::new();
    for entry in entries {
        // Older receipts can contain multiple appends to the same plugin list.
        // Its first baseline and final hash describe the actual owned change.
        previous_entries
            .entry(entry.path.clone())
            .and_modify(|prior| prior.value_sha256.clone_from(&entry.value_sha256))
            .or_insert_with(|| entry.clone());
    }
    previous_entries
}
