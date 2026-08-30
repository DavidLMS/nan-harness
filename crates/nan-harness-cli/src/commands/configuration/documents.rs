use super::*;

pub(super) fn prepare_documents(
    plans: &[DocumentPlan],
    previous: Option<&[DocumentReceipt]>,
) -> Result<Vec<PreparedDocument>, ConfigurationError> {
    let mut matched = BTreeSet::new();
    let prepared = plans
        .iter()
        .map(|plan| {
            let previous = previous.and_then(|receipts| {
                receipts.iter().enumerate().find_map(|(index, receipt)| {
                    (!matched.contains(&index) && plan_matches_receipt(plan, receipt))
                        .then_some((index, receipt))
                })
            });
            if let Some((index, _)) = previous {
                matched.insert(index);
            }
            let previous = previous.map(|(_, receipt)| receipt);
            match (plan, previous) {
                (DocumentPlan::Json(plan), None) => prepare_json(plan, None),
                (DocumentPlan::Json(plan), Some(DocumentReceipt::Json(receipt))) => {
                    prepare_json(plan, Some(receipt))
                }
                (DocumentPlan::Yaml(plan), None) => prepare_yaml(plan, None),
                (DocumentPlan::Yaml(plan), Some(DocumentReceipt::Yaml(receipt))) => {
                    prepare_yaml(plan, Some(&PreviousYamlReceipt::Yaml(receipt)))
                }
                (DocumentPlan::Yaml(plan), Some(DocumentReceipt::TextBlock(receipt))) => {
                    prepare_yaml(plan, Some(&PreviousYamlReceipt::TextBlock(receipt)))
                }
                (DocumentPlan::TextBlock(plan), None) => prepare_text_block(plan, None),
                (DocumentPlan::TextBlock(plan), Some(DocumentReceipt::TextBlock(receipt))) => {
                    prepare_text_block(plan, Some(receipt))
                }
                (DocumentPlan::ExactFile(plan), None) => prepare_exact_file(plan, None),
                (DocumentPlan::ExactFile(plan), Some(DocumentReceipt::ExactFile(receipt))) => {
                    prepare_exact_file(plan, Some(receipt))
                }
                (DocumentPlan::Kimi(plan), None) => prepare_kimi(plan, None),
                (DocumentPlan::Kimi(plan), Some(DocumentReceipt::Toml(receipt))) => {
                    prepare_kimi(plan, Some(receipt))
                }
                _ => Err(ConfigurationError::ReceiptMismatch),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if previous.is_some_and(|receipts| matched.len() != receipts.len()) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    Ok(prepared)
}

fn plan_matches_receipt(plan: &DocumentPlan, receipt: &DocumentReceipt) -> bool {
    match (plan, receipt) {
        (DocumentPlan::Json(plan), DocumentReceipt::Json(receipt)) => plan.path == receipt.path,
        (DocumentPlan::Yaml(plan), DocumentReceipt::Yaml(receipt)) => plan.path == receipt.path,
        (DocumentPlan::Yaml(plan), DocumentReceipt::TextBlock(receipt)) => {
            plan.path == receipt.path
                && plan.legacy_block.as_ref().is_some_and(|legacy| {
                    legacy.begin == receipt.begin && legacy.end == receipt.end
                })
        }
        (DocumentPlan::TextBlock(plan), DocumentReceipt::TextBlock(receipt)) => {
            plan.path == receipt.path && plan.begin == receipt.begin && plan.end == receipt.end
        }
        (DocumentPlan::ExactFile(plan), DocumentReceipt::ExactFile(receipt)) => {
            plan.path == receipt.path
        }
        (DocumentPlan::Kimi(plan), DocumentReceipt::Toml(receipt)) => plan.path == receipt.path,
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(super) enum PreviousYamlReceipt<'a> {
    Yaml(&'a YamlReceipt),
    TextBlock(&'a TextBlockReceipt),
}

pub(super) fn prepare_yaml(
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

fn prepare_yaml_entries(
    document: &mut YamlValue,
    plan: &YamlPlan,
    previous: Option<&YamlReceipt>,
) -> Result<Vec<YamlEntryReceipt>, ConfigurationError> {
    let previous_entries = previous.map_or_else(BTreeMap::new, |receipt| {
        receipt
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect::<BTreeMap<_, _>>()
    });
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
    let mut entries = Vec::with_capacity(plan.entries.len());
    for planned in &plan.entries {
        let prior = previous_entries.get(&planned.path).copied();
        let current = get_yaml_path(document, &planned.path).cloned();
        if prior.is_none() && matches!(planned.mode, YamlEntryMode::Exclusive) && current.is_some()
        {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        let previous_value = prior.and_then(|entry| entry.previous.clone()).or_else(|| {
            matches!(
                planned.mode,
                YamlEntryMode::Override | YamlEntryMode::AppendUnique
            )
            .then_some(current.clone())
            .flatten()
        });
        let desired = match planned.mode {
            YamlEntryMode::AppendUnique => append_unique_yaml_value(
                current.as_ref(),
                &planned.value,
                &plan.path,
                planned.path.last().map_or("", String::as_str),
            )?,
            YamlEntryMode::Exclusive | YamlEntryMode::Override => planned.value.clone(),
        };
        set_yaml_path(document, &planned.path, desired.clone(), &plan.path)?;
        entries.push(YamlEntryReceipt {
            path: planned.path.clone(),
            value_sha256: hash_yaml(&desired)?,
            previous: previous_value,
        });
    }
    Ok(entries)
}

fn remove_managed_text_block(
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

pub(super) fn prepare_json(
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

fn prepare_json_entries(
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

pub(super) fn prepare_text_block(
    plan: &TextBlockPlan,
    previous: Option<&TextBlockReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| {
        receipt.path != plan.path || receipt.begin != plan.begin || receipt.end != plan.end
    }) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    if plan.body.is_none() {
        return prepare_inactive_text_block(plan, previous);
    }
    let previous = previous.filter(|receipt| receipt.active);
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let source = match original.as_deref() {
        Some(contents) => String::from_utf8(contents.to_vec()).map_err(|source| {
            ConfigurationError::InvalidUtf8 {
                path: plan.path.clone(),
                source,
            }
        })?,
        None => String::new(),
    };
    let desired = format!(
        "{}\n{}\n{}\n",
        plan.begin,
        plan.body.as_deref().unwrap_or_default(),
        plan.end
    );
    let range = block_range(&source, &plan.begin, &plan.end)?;
    let replacement = if let Some(range) = range {
        let receipt = previous
            .ok_or_else(|| ConfigurationError::UnmanagedDocumentConflict(plan.path.clone()))?;
        if sha256(source[range.clone()].as_bytes()) != receipt.block_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
        let mut rendered = source;
        rendered.replace_range(range, &desired);
        rendered
    } else {
        if previous.is_some() {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
        for key in &plan.conflicting_keys {
            if source
                .lines()
                .any(|line| line.trim_start().starts_with(key))
            {
                return Err(ConfigurationError::UnmanagedDocumentConflict(
                    plan.path.clone(),
                ));
            }
        }
        let mut rendered = source;
        if !rendered.is_empty() && !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(&desired);
        rendered
    };
    let created_file = previous.map_or(original.is_none(), |receipt| receipt.created_file);
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement: Some(replacement.into_bytes()),
        receipt: DocumentReceipt::TextBlock(TextBlockReceipt {
            path: plan.path.clone(),
            created_file,
            begin: plan.begin.clone(),
            end: plan.end.clone(),
            block_sha256: sha256(desired.as_bytes()),
            active: true,
        }),
    })
}

fn prepare_inactive_text_block(
    plan: &TextBlockPlan,
    previous: Option<&TextBlockReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if let Some(receipt) = previous.filter(|receipt| receipt.active) {
        let mut prepared = prepare_text_block_removal(receipt)?;
        prepared.receipt = DocumentReceipt::TextBlock(TextBlockReceipt {
            path: plan.path.clone(),
            created_file: receipt.created_file,
            begin: plan.begin.clone(),
            end: plan.end.clone(),
            block_sha256: String::new(),
            active: false,
        });
        return Ok(prepared);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    Ok(PreparedDocument {
        path: plan.path.clone(),
        replacement: original.clone(),
        original,
        permissions,
        receipt: DocumentReceipt::TextBlock(TextBlockReceipt {
            path: plan.path.clone(),
            created_file: false,
            begin: plan.begin.clone(),
            end: plan.end.clone(),
            block_sha256: String::new(),
            active: false,
        }),
    })
}

pub(super) fn prepare_exact_file(
    plan: &ExactFilePlan,
    previous: Option<&ExactFileReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| receipt.path != plan.path) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    match (original.as_deref(), previous, plan.payload.as_ref()) {
        (Some(_), None, Some(_)) => {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        (Some(contents), Some(receipt), _)
            if receipt.active && sha256(contents) != receipt.sha256 =>
        {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
        (None, Some(receipt), _) if receipt.active => {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
        (Some(_), Some(receipt), Some(_)) if !receipt.active => {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        _ => {}
    }
    let active = plan.payload.is_some();
    let replacement = if active {
        plan.payload.clone()
    } else if previous.is_some_and(|receipt| receipt.active) {
        None
    } else {
        original.clone()
    };
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::ExactFile(ExactFileReceipt {
            path: plan.path.clone(),
            sha256: plan.payload.as_deref().map_or_else(String::new, sha256),
            active,
        }),
    })
}

pub(super) fn prepare_kimi(
    plan: &KimiPlan,
    previous: Option<&TomlReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| receipt.path != plan.path) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let source = original
        .as_deref()
        .map(|contents| String::from_utf8_lossy(contents).into_owned())
        .unwrap_or_default();
    let mut document = if source.trim().is_empty() {
        DocumentMut::new()
    } else {
        source
            .parse::<DocumentMut>()
            .map_err(|source| ConfigurationError::ParseToml {
                path: plan.path.clone(),
                source,
            })?
    };
    if let Some(receipt) = previous
        && !kimi_receipt_is_active(&document, receipt)
    {
        return Err(ConfigurationError::ManagedDocumentChanged(
            plan.path.clone(),
        ));
    }

    let previous_default_model = match previous {
        Some(receipt) => receipt.previous_default_model.clone(),
        None => match document.get("default_model") {
            Some(item) => Some(
                item.as_str()
                    .ok_or_else(|| ConfigurationError::TomlFieldNotString {
                        path: plan.path.clone(),
                        field: "default_model".to_owned(),
                    })?
                    .to_owned(),
            ),
            None => None,
        },
    };

    let providers = table_mut_or_create(&mut document, "providers", &plan.path)?;
    if previous.is_none() && providers.contains_key("nan") {
        return Err(ConfigurationError::UnmanagedDocumentConflict(
            plan.path.clone(),
        ));
    }
    let mut provider = Table::new();
    provider.insert("type", value("openai_legacy"));
    provider.insert("base_url", value(&plan.base_url));
    provider.insert("api_key", value(&plan.api_key));
    providers.insert("nan", Item::Table(provider));

    let desired_model_names = configure_kimi_models(&mut document, plan, previous)?;
    let selected_model = kimi_model_name(&plan.default_model);
    document["default_model"] = value(&selected_model);

    let provider_sha256 = hash_toml_item(&document["providers"]["nan"])?;
    let model_hashes = desired_model_names
        .into_iter()
        .map(|name| hash_toml_item(&document["models"][&name]).map(|hash| (name, hash)))
        .collect::<Result<_, _>>()?;
    let default_model_sha256 = hash_toml_item(&document["default_model"])?;
    let replacement = document.to_string().into_bytes();
    let created_file = previous.map_or(source.is_empty(), |receipt| receipt.created_file);
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement: Some(replacement),
        receipt: DocumentReceipt::Toml(TomlReceipt {
            path: plan.path.clone(),
            created_file,
            provider_sha256,
            models: model_hashes,
            default_model_sha256,
            previous_default_model,
        }),
    })
}

fn configure_kimi_models(
    document: &mut DocumentMut,
    plan: &KimiPlan,
    previous: Option<&TomlReceipt>,
) -> Result<BTreeSet<String>, ConfigurationError> {
    let desired_names = plan
        .models
        .iter()
        .map(|model| kimi_model_name(&model.id))
        .collect::<BTreeSet<_>>();
    let models = table_mut_or_create(document, "models", &plan.path)?;
    if let Some(receipt) = previous {
        for name in receipt.models.keys() {
            if !desired_names.contains(name) {
                models.remove(name);
            }
        }
    }
    for model in &plan.models {
        let name = kimi_model_name(&model.id);
        if previous.is_none() && models.contains_key(&name) {
            return Err(ConfigurationError::UnmanagedDocumentConflict(
                plan.path.clone(),
            ));
        }
        let mut item = Table::new();
        item.insert("provider", value("nan"));
        item.insert("model", value(&model.id));
        item.insert(
            "max_context_size",
            value(toml_integer(
                model.context_window,
                "context window",
                &model.id,
            )?),
        );
        models.insert(&name, Item::Table(item));
    }
    Ok(desired_names)
}

pub(super) fn prepare_removals(
    receipts: &[DocumentReceipt],
) -> Result<Vec<PreparedDocument>, ConfigurationError> {
    receipts
        .iter()
        .map(|receipt| match receipt {
            DocumentReceipt::Json(receipt) => prepare_json_removal(receipt),
            DocumentReceipt::Yaml(receipt) => prepare_yaml_removal(receipt),
            DocumentReceipt::TextBlock(receipt) => prepare_text_block_removal(receipt),
            DocumentReceipt::ExactFile(receipt) => prepare_exact_file_removal(receipt),
            DocumentReceipt::Toml(receipt) => prepare_kimi_removal(receipt),
        })
        .collect()
}

pub(super) fn prepare_yaml_removal(
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
    for entry in &receipt.entries {
        let current = get_yaml_path(&document, &entry.path)
            .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
        if hash_yaml(current)? != entry.value_sha256 {
            return Err(ConfigurationError::ManagedDocumentChanged(
                receipt.path.clone(),
            ));
        }
    }
    for entry in receipt.entries.iter().rev() {
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

pub(super) fn prepare_json_removal(
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

pub(super) fn prepare_text_block_removal(
    receipt: &TextBlockReceipt,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&receipt.path)?;
    let permissions = file_permissions(&receipt.path)?;
    if !receipt.active {
        return Ok(PreparedDocument {
            path: receipt.path.clone(),
            replacement: original.clone(),
            original,
            permissions,
            receipt: DocumentReceipt::TextBlock(receipt.clone()),
        });
    }
    let Some(contents) = original.as_deref() else {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    };
    let source =
        String::from_utf8(contents.to_vec()).map_err(|source| ConfigurationError::InvalidUtf8 {
            path: receipt.path.clone(),
            source,
        })?;
    let range = block_range(&source, &receipt.begin, &receipt.end)?
        .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
    if sha256(source[range.clone()].as_bytes()) != receipt.block_sha256 {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    let mut rendered = source;
    let start = if range.start > 0 && rendered.as_bytes().get(range.start - 1) == Some(&b'\n') {
        range.start - 1
    } else {
        range.start
    };
    rendered.replace_range(start..range.end, "");
    let replacement = if receipt.created_file && rendered.is_empty() {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::TextBlock(receipt.clone()),
    })
}

pub(super) fn prepare_exact_file_removal(
    receipt: &ExactFileReceipt,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&receipt.path)?;
    let permissions = file_permissions(&receipt.path)?;
    if !receipt.active {
        return Ok(PreparedDocument {
            path: receipt.path.clone(),
            replacement: original.clone(),
            original,
            permissions,
            receipt: DocumentReceipt::ExactFile(receipt.clone()),
        });
    }
    let Some(contents) = original.as_deref() else {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    };
    if sha256(contents) != receipt.sha256 {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement: None,
        receipt: DocumentReceipt::ExactFile(receipt.clone()),
    })
}

pub(super) fn prepare_kimi_removal(
    receipt: &TomlReceipt,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&receipt.path)?;
    let permissions = file_permissions(&receipt.path)?;
    let Some(contents) = original.as_deref() else {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    };
    let source = String::from_utf8_lossy(contents);
    let mut document =
        source
            .parse::<DocumentMut>()
            .map_err(|source| ConfigurationError::ParseToml {
                path: receipt.path.clone(),
                source,
            })?;
    if !kimi_receipt_is_active(&document, receipt) {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    remove_toml_child(&mut document, "providers", "nan", &receipt.path)?;
    for name in receipt.models.keys() {
        remove_toml_child(&mut document, "models", name, &receipt.path)?;
    }
    if let Some(previous) = &receipt.previous_default_model {
        document["default_model"] = value(previous);
    } else {
        document.remove("default_model");
    }
    remove_empty_toml_table(&mut document, "providers");
    remove_empty_toml_table(&mut document, "models");
    let replacement = if receipt.created_file && document.as_table().is_empty() {
        None
    } else {
        Some(document.to_string().into_bytes())
    };
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::Toml(receipt.clone()),
    })
}

pub(super) fn apply_prepared(documents: &[PreparedDocument]) -> Result<(), ConfigurationError> {
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

pub(super) fn rollback_prepared(documents: &[PreparedDocument]) {
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

pub(super) fn document_is_active(receipt: &DocumentReceipt) -> bool {
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

pub(super) fn kimi_receipt_is_active(document: &DocumentMut, receipt: &TomlReceipt) -> bool {
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

pub(super) fn table_mut_or_create<'a>(
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

pub(super) fn remove_toml_child(
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

pub(super) fn remove_empty_toml_table(document: &mut DocumentMut, name: &str) {
    if document[name].as_table().is_some_and(Table::is_empty) {
        document.remove(name);
    }
}

pub(super) fn hash_toml_item(item: &Item) -> Result<String, ConfigurationError> {
    let mut wrapper = DocumentMut::new();
    wrapper["value"] = item.clone();
    let semantic = toml_edit::de::from_document::<Value>(wrapper)
        .map_err(ConfigurationError::NormalizeToml)?;
    hash_json(&semantic["value"])
}

fn toml_integer(value: u64, field: &'static str, model: &str) -> Result<i64, ConfigurationError> {
    i64::try_from(value).map_err(|_| ConfigurationError::ModelValueOutOfRange {
        field,
        model: model.to_owned(),
    })
}

pub(super) fn kimi_model_name(model_id: &str) -> String {
    format!("nan/{model_id}")
}

pub(super) fn set_json_path(
    document: &mut Value,
    path: &[String],
    value: Value,
    document_path: &Path,
) -> Result<(), ConfigurationError> {
    let Some((last, parents)) = path.split_last() else {
        return Err(ConfigurationError::InvalidManagedPath);
    };
    let mut current = document;
    for segment in parents {
        let object =
            current
                .as_object_mut()
                .ok_or_else(|| ConfigurationError::DocumentFieldNotObject {
                    path: document_path.to_path_buf(),
                    field: segment.clone(),
                })?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
        .as_object_mut()
        .ok_or_else(|| ConfigurationError::DocumentFieldNotObject {
            path: document_path.to_path_buf(),
            field: last.clone(),
        })?
        .insert(last.clone(), value);
    Ok(())
}

pub(super) fn get_json_path<'a>(document: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(document, |current, segment| {
        current.as_object()?.get(segment)
    })
}

pub(super) fn remove_json_path(document: &mut Value, path: &[String]) {
    if path.is_empty() {
        return;
    }
    remove_json_path_inner(document, path);
}

pub(super) fn remove_json_path_inner(document: &mut Value, path: &[String]) -> bool {
    let Some(object) = document.as_object_mut() else {
        return false;
    };
    if path.len() == 1 {
        object.remove(&path[0]);
    } else if let Some(child) = object.get_mut(&path[0])
        && remove_json_path_inner(child, &path[1..])
    {
        object.remove(&path[0]);
    }
    object.is_empty()
}

pub(super) fn set_yaml_path(
    document: &mut YamlValue,
    path: &[String],
    value: YamlValue,
    document_path: &Path,
) -> Result<(), ConfigurationError> {
    let Some((last, parents)) = path.split_last() else {
        return Err(ConfigurationError::InvalidManagedPath);
    };
    let mut current = document;
    for segment in parents {
        let mapping =
            current
                .as_mapping_mut()
                .ok_or_else(|| ConfigurationError::YamlFieldNotMapping {
                    path: document_path.to_path_buf(),
                    field: segment.clone(),
                })?;
        current = mapping
            .entry(YamlValue::String(segment.clone()))
            .or_insert_with(|| YamlValue::Mapping(serde_yaml_ng::Mapping::default()));
    }
    current
        .as_mapping_mut()
        .ok_or_else(|| ConfigurationError::YamlFieldNotMapping {
            path: document_path.to_path_buf(),
            field: last.clone(),
        })?
        .insert(YamlValue::String(last.clone()), value);
    Ok(())
}

pub(super) fn get_yaml_path<'a>(document: &'a YamlValue, path: &[String]) -> Option<&'a YamlValue> {
    path.iter().try_fold(document, |current, segment| {
        current
            .as_mapping()?
            .get(YamlValue::String(segment.clone()))
    })
}

pub(super) fn remove_yaml_path(document: &mut YamlValue, path: &[String]) {
    if path.is_empty() {
        return;
    }
    remove_yaml_path_inner(document, path);
}

fn remove_yaml_path_inner(document: &mut YamlValue, path: &[String]) -> bool {
    let Some(mapping) = document.as_mapping_mut() else {
        return false;
    };
    let key = YamlValue::String(path[0].clone());
    if path.len() == 1 {
        mapping.remove(&key);
    } else if let Some(child) = mapping.get_mut(&key)
        && remove_yaml_path_inner(child, &path[1..])
    {
        mapping.remove(&key);
    }
    mapping.is_empty()
}

fn append_unique_yaml_value(
    current: Option<&YamlValue>,
    value: &YamlValue,
    path: &Path,
    field: &str,
) -> Result<YamlValue, ConfigurationError> {
    let mut values = match current {
        Some(YamlValue::Sequence(values)) => values.clone(),
        Some(_) => {
            return Err(ConfigurationError::YamlFieldNotSequence {
                path: path.to_path_buf(),
                field: field.to_owned(),
            });
        }
        None => Vec::new(),
    };
    if !values.contains(value) {
        values.push(value.clone());
    }
    Ok(YamlValue::Sequence(values))
}

fn append_unique_json_value(
    current: Option<&Value>,
    value: &Value,
    path: &Path,
    field: &str,
) -> Result<Value, ConfigurationError> {
    let mut values = match current {
        Some(Value::Array(values)) => values.clone(),
        Some(_) => {
            return Err(ConfigurationError::DocumentFieldNotArray {
                path: path.to_path_buf(),
                field: field.to_owned(),
            });
        }
        None => Vec::new(),
    };
    if !values.contains(value) {
        values.push(value.clone());
    }
    Ok(Value::Array(values))
}

fn hash_yaml(value: &YamlValue) -> Result<String, ConfigurationError> {
    serde_yaml_ng::to_string(value)
        .map(|rendered| sha256(rendered.as_bytes()))
        .map_err(ConfigurationError::SerializeYaml)
}

pub(super) fn block_range(
    source: &str,
    begin: &str,
    end: &str,
) -> Result<Option<std::ops::Range<usize>>, ConfigurationError> {
    let starts = source.match_indices(begin).collect::<Vec<_>>();
    let ends = source.match_indices(end).collect::<Vec<_>>();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, _)], [(end_start, _)]) if start < end_start => {
            let mut end_index = end_start + end.len();
            if source.as_bytes().get(end_index) == Some(&b'\n') {
                end_index += 1;
            }
            Ok(Some(*start..end_index))
        }
        _ => Err(ConfigurationError::InvalidManagedBlock),
    }
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ConfigurationError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigurationError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn file_permissions(path: &Path) -> Result<Option<Permissions>, ConfigurationError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigurationError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn remove_optional_file(path: &Path) -> Result<(), ConfigurationError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigurationError::RemoveDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn hash_json(value: &Value) -> Result<String, ConfigurationError> {
    serde_json::to_vec(value)
        .map(|payload| sha256(&payload))
        .map_err(ConfigurationError::SerializeDocument)
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

pub(super) fn yaml_quote(value: &str) -> Result<String, ConfigurationError> {
    serde_json::to_string(value).map_err(ConfigurationError::SerializeDocument)
}

pub(super) fn dotenv_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
