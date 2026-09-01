use super::*;

pub(crate) fn prepare_kimi(
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

pub(crate) fn configure_kimi_models(
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

pub(crate) fn prepare_kimi_removal(
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
