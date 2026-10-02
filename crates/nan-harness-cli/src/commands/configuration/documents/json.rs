use super::*;

type JsonEntryIdentity = (Vec<String>, Option<BTreeMap<String, String>>);

pub(crate) fn prepare_json(
    plan: &JsonPlan,
    previous: Option<&JsonReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    prepare_json_format(plan, previous, false)
}

pub(crate) fn prepare_jsonc(
    plan: &JsonPlan,
    previous: Option<&JsonReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    prepare_json_format(plan, previous, true)
}

fn prepare_json_format(
    plan: &JsonPlan,
    previous: Option<&JsonReceipt>,
    comments: bool,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| receipt.path != plan.path || receipt.comments != comments) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let mut document = match original.as_deref() {
        Some(contents) => parse_json_document(contents, &plan.path, comments)?,
        None => Value::Object(Map::new()),
    };
    if !document.is_object() {
        return Err(ConfigurationError::DocumentRootNotObject(plan.path.clone()));
    }
    let entries = prepare_json_entries(&mut document, plan, previous)?;
    let created_file = previous.map_or(original.is_none(), |receipt| receipt.created_file);
    let replacement =
        if entries.is_empty() && previous.is_none_or(|receipt| receipt.entries.is_empty()) {
            original.clone()
        } else {
            json_replacement(
                &document,
                original.as_deref(),
                &plan.path,
                comments,
                created_file,
            )?
        };
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Json(JsonReceipt {
            comments,
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
        let current =
            get_json_entry(document, &prior.path, prior.selector.as_ref(), &plan.path)?
                .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(plan.path.clone()))?;
        if !json_entry_matches(current, prior)? {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
    }
    let desired_paths = plan
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), entry.selector.clone()))
        .collect::<BTreeSet<_>>();
    for prior in previous_entries
        .values()
        .filter(|entry| !desired_paths.contains(&(entry.path.clone(), entry.selector.clone())))
        .rev()
    {
        restore_json_entry(document, prior, &plan.path)?;
    }
    let created_arrays = plan
        .entries
        .iter()
        .filter(|entry| entry.selector.is_some())
        .filter(|entry| {
            get_json_path(document, &entry.path).is_none()
                || previous_entries
                    .values()
                    .any(|prior| prior.path == entry.path && prior.created_array)
        })
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let mut entries: Vec<JsonEntryReceipt> = Vec::with_capacity(plan.entries.len());
    for planned in &plan.entries {
        let prior = previous_entries.get(&(planned.path.clone(), planned.selector.clone()));
        let existing = entries
            .iter()
            .position(|entry| entry.path == planned.path && entry.selector == planned.selector);
        let receipt = prepare_json_entry(
            document,
            plan,
            planned,
            prior,
            existing.is_some(),
            created_arrays.contains(&planned.path),
        )?;
        if let Some(index) = existing {
            entries[index].value_sha256 = receipt.value_sha256;
        } else {
            entries.push(receipt);
        }
    }
    Ok(entries)
}

fn prepare_json_entry(
    document: &mut Value,
    plan: &JsonPlan,
    planned: &plans::JsonEntryPlan,
    prior: Option<&JsonEntryReceipt>,
    already_appended: bool,
    created_array: bool,
) -> Result<JsonEntryReceipt, ConfigurationError> {
    let current = get_json_entry(
        document,
        &planned.path,
        planned.selector.as_ref(),
        &plan.path,
    )?
    .cloned();
    if prior.is_none() && matches!(planned.mode, JsonEntryMode::Exclusive) && current.is_some() {
        return Err(ConfigurationError::UnmanagedDocumentConflict(
            plan.path.clone(),
        ));
    }
    let previous_value = match prior {
        Some(entry) => entry.previous.clone(),
        None => matches!(
            planned.mode,
            JsonEntryMode::Override | JsonEntryMode::AppendUnique | JsonEntryMode::EnsureArray
        )
        .then_some(current.clone())
        .flatten(),
    };
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
        JsonEntryMode::EnsureArray => match current {
            None => Value::Array(Vec::new()),
            Some(value) if value.is_array() => value,
            Some(_) => {
                return Err(ConfigurationError::DocumentFieldNotArray {
                    path: plan.path.clone(),
                    field: planned.path.last().cloned().unwrap_or_default(),
                });
            }
        },
        JsonEntryMode::Exclusive | JsonEntryMode::Override => planned.value.clone(),
    };
    set_json_entry(
        document,
        &planned.path,
        planned.selector.as_ref(),
        desired.clone(),
        &plan.path,
    )?;
    Ok(JsonEntryReceipt {
        path: planned.path.clone(),
        selector: planned.selector.clone(),
        created_array,
        container_only: matches!(planned.mode, JsonEntryMode::EnsureArray),
        value_sha256: hash_json(&desired)?,
        previous: previous_value,
    })
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
    let mut document = parse_json_document(contents, &receipt.path, receipt.comments)?;
    let entries = merged_json_receipts(&receipt.entries);
    for entry in entries.values() {
        let current = get_json_entry(
            &document,
            &entry.path,
            entry.selector.as_ref(),
            &receipt.path,
        )?
        .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
        if !json_entry_matches(current, entry)? {
            return Err(ConfigurationError::ManagedDocumentChanged(
                receipt.path.clone(),
            ));
        }
    }
    for entry in entries.values().rev() {
        restore_json_entry(&mut document, entry, &receipt.path)?;
    }
    let replacement = json_replacement(
        &document,
        original.as_deref(),
        &receipt.path,
        receipt.comments,
        receipt.created_file,
    )?;
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Json(receipt.clone()),
    })
}

fn merged_json_receipts(
    entries: &[JsonEntryReceipt],
) -> BTreeMap<JsonEntryIdentity, JsonEntryReceipt> {
    let mut previous_entries: BTreeMap<JsonEntryIdentity, JsonEntryReceipt> = BTreeMap::new();
    for entry in entries {
        // Older receipts can contain multiple appends to the same plugin list.
        // Its first baseline and final hash describe the actual owned change.
        previous_entries
            .entry((entry.path.clone(), entry.selector.clone()))
            .and_modify(|prior| prior.value_sha256.clone_from(&entry.value_sha256))
            .or_insert_with(|| entry.clone());
    }
    previous_entries
}

// Required array containers belong to the schema; their members remain user-owned.
pub(super) fn json_entry_matches(
    value: &Value,
    entry: &JsonEntryReceipt,
) -> Result<bool, ConfigurationError> {
    if entry.container_only {
        Ok(value.is_array())
    } else {
        Ok(hash_json(value)? == entry.value_sha256)
    }
}
