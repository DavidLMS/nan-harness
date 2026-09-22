use super::*;

pub(crate) fn prepare_yaml(
    plan: &YamlPlan,
    previous: Option<&PreviousYamlReceipt<'_>>,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let mut source = original
        .as_deref()
        .map(|contents| String::from_utf8(contents.to_vec()))
        .transpose()
        .map_err(|source| ConfigurationError::InvalidUtf8 {
            path: plan.path.clone(),
            source,
        })?
        .unwrap_or_default();
    let (previous, created_file) = match previous {
        Some(PreviousYamlReceipt::Yaml(receipt)) => {
            if receipt.path != plan.path {
                return Err(ConfigurationError::ReceiptMismatch);
            }
            (Some(*receipt), receipt.created_file)
        }
        Some(PreviousYamlReceipt::TextBlock(receipt)) => {
            let legacy = plan
                .legacy_block
                .as_ref()
                .ok_or(ConfigurationError::ReceiptMismatch)?;
            if receipt.path != plan.path
                || receipt.begin != legacy.begin
                || receipt.end != legacy.end
            {
                return Err(ConfigurationError::ReceiptMismatch);
            }
            source = remove_managed_text_block(&source, receipt)?;
            (None, receipt.created_file)
        }
        None => (None, original.is_none()),
    };
    let mut document = if source.trim().is_empty() {
        YamlValue::Mapping(serde_yaml_ng::Mapping::default())
    } else {
        serde_yaml_ng::from_str::<YamlValue>(&source).map_err(|source| {
            ConfigurationError::ParseYaml {
                path: plan.path.clone(),
                source,
            }
        })?
    };
    if !document.is_mapping() {
        return Err(ConfigurationError::YamlRootNotMapping(plan.path.clone()));
    }
    let entries = prepare_yaml_entries(&mut document, plan, previous)?;
    let replacement = if entries.is_empty()
        && previous.is_none_or(|receipt| receipt.entries.is_empty())
        && plan.legacy_block.is_none()
    {
        original.clone()
    } else if created_file
        && document
            .as_mapping()
            .is_some_and(serde_yaml_ng::Mapping::is_empty)
    {
        None
    } else {
        Some(
            serde_yaml_ng::to_string(&document)
                .map_err(ConfigurationError::SerializeYaml)?
                .into_bytes(),
        )
    };
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Yaml(YamlReceipt {
            path: plan.path.clone(),
            created_file,
            entries,
        }),
    })
}

pub(crate) fn prepare_yaml_entries(
    document: &mut YamlValue,
    plan: &YamlPlan,
    previous: Option<&YamlReceipt>,
) -> Result<Vec<YamlEntryReceipt>, ConfigurationError> {
    let previous_entries = merged_yaml_receipts(previous.map_or(&[], |receipt| &receipt.entries));
    for prior in previous_entries.values() {
        let current = get_yaml_path(document, &prior.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(plan.path.clone()))?;
        if hash_yaml(current)? != prior.value_sha256 {
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
            set_yaml_path(document, &prior.path, value.clone(), &plan.path)?;
        } else {
            remove_yaml_path(document, &prior.path);
        }
    }
    let mut entries: Vec<YamlEntryReceipt> = Vec::with_capacity(plan.entries.len());
    for planned in &plan.entries {
        let prior = previous_entries.get(&planned.path);
        let current = get_yaml_path(document, &planned.path).cloned();
        if prior.is_none() && matches!(planned.mode, YamlEntryMode::Exclusive) && current.is_some()
        {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        let previous_value = match prior {
            Some(entry) => entry.previous.clone(),
            None => matches!(
                planned.mode,
                YamlEntryMode::Override | YamlEntryMode::AppendUnique
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
            YamlEntryMode::AppendUnique => append_unique_yaml_value(
                append_base,
                &planned.value,
                &plan.path,
                planned.path.last().map_or("", String::as_str),
            )?,
            YamlEntryMode::Exclusive | YamlEntryMode::Override => planned.value.clone(),
        };
        set_yaml_path(document, &planned.path, desired.clone(), &plan.path)?;
        if let Some(entry) = entries.iter_mut().find(|entry| entry.path == planned.path) {
            entry.value_sha256 = hash_yaml(&desired)?;
        } else {
            entries.push(YamlEntryReceipt {
                path: planned.path.clone(),
                value_sha256: hash_yaml(&desired)?,
                previous: previous_value,
            });
        }
    }
    Ok(entries)
}

pub(crate) fn remove_managed_text_block(
    source: &str,
    receipt: &TextBlockReceipt,
) -> Result<String, ConfigurationError> {
    let range = block_range(source, &receipt.begin, &receipt.end)?
        .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
    if sha256(source[range.clone()].as_bytes()) != receipt.block_sha256 {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    let mut rendered = source.to_owned();
    let start = if range.start > 0 && rendered.as_bytes().get(range.start - 1) == Some(&b'\n') {
        range.start - 1
    } else {
        range.start
    };
    rendered.replace_range(start..range.end, "");
    Ok(rendered)
}

pub(crate) fn prepare_yaml_removal(
    receipt: &YamlReceipt,
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
                receipt: DocumentReceipt::Yaml(receipt.clone()),
            });
        }
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    };
    let mut document = serde_yaml_ng::from_slice::<YamlValue>(contents).map_err(|source| {
        ConfigurationError::ParseYaml {
            path: receipt.path.clone(),
            source,
        }
    })?;
    let entries = merged_yaml_receipts(&receipt.entries);
    for entry in entries.values() {
        let current = get_yaml_path(&document, &entry.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
        if hash_yaml(current)? != entry.value_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                receipt.path.clone(),
            ));
        }
    }
    for entry in entries.values().rev() {
        if let Some(previous) = &entry.previous {
            set_yaml_path(&mut document, &entry.path, previous.clone(), &receipt.path)?;
        } else {
            remove_yaml_path(&mut document, &entry.path);
        }
    }
    let replacement = if receipt.created_file
        && document
            .as_mapping()
            .is_some_and(serde_yaml_ng::Mapping::is_empty)
    {
        None
    } else {
        Some(
            serde_yaml_ng::to_string(&document)
                .map_err(ConfigurationError::SerializeYaml)?
                .into_bytes(),
        )
    };
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Yaml(receipt.clone()),
    })
}

fn merged_yaml_receipts(entries: &[YamlEntryReceipt]) -> BTreeMap<Vec<String>, YamlEntryReceipt> {
    let mut previous_entries: BTreeMap<Vec<String>, YamlEntryReceipt> = BTreeMap::new();
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
