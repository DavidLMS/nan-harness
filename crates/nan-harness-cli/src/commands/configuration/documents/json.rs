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
    let previous_entries = previous.map_or_else(BTreeMap::new, |receipt| {
        receipt
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect::<BTreeMap<_, _>>()
    });
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
    let mut entries = Vec::with_capacity(plan.entries.len());
    for planned in &plan.entries {
        let prior = previous_entries.get(&planned.path).copied();
        if prior.is_none()
            && matches!(planned.mode, JsonEntryMode::Exclusive)
            && get_json_path(document, &planned.path).is_some()
        {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        let current = get_json_path(document, &planned.path).cloned();
        let prior_value = prior.and_then(|entry| entry.previous.clone()).or_else(|| {
            matches!(
                planned.mode,
                JsonEntryMode::Override | JsonEntryMode::AppendUnique
            )
            .then_some(current.clone())
            .flatten()
        });
        let desired = match planned.mode {
            JsonEntryMode::AppendUnique => append_unique_json_value(
                current.as_ref(),
                &planned.value,
                &plan.path,
                planned.path.last().map_or("", String::as_str),
            )?,
            JsonEntryMode::Exclusive | JsonEntryMode::Override => planned.value.clone(),
        };
        set_json_path(document, &planned.path, desired.clone(), &plan.path)?;
        entries.push(JsonEntryReceipt {
            path: planned.path.clone(),
            value_sha256: hash_json(&desired)?,
            previous: prior_value,
        });
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
    for entry in &receipt.entries {
        let current = get_json_path(&document, &entry.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
        if hash_json(current)? != entry.value_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                receipt.path.clone(),
            ));
        }
    }
    for entry in receipt.entries.iter().rev() {
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
