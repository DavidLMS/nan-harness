use super::{
    ConfigurationError, TextBlockReceipt, YamlEntryReceipt, YamlPlan, YamlReceipt, YamlValue,
    hash_yaml, sha256, yaml_quote,
};

/// DSH rewrites old flat credentials into version 1 refs. Authenticate ownership by
/// reconstructing the old generated block from the actual scalar, not by its indentation.
pub(super) fn deepseek_credential_receipt(
    receipt: &TextBlockReceipt,
    contents: &[u8],
) -> Result<Option<YamlReceipt>, ConfigurationError> {
    if receipt
        .path
        .file_name()
        .is_none_or(|name| name != ".credentials.yaml")
        || receipt.begin != "# nan-harness:begin provider-credential"
        || receipt.end != "# nan-harness:end provider-credential"
        || !receipt.active
    {
        return Ok(None);
    }
    let document: YamlValue =
        serde_yaml_ng::from_slice(contents).map_err(|source| ConfigurationError::ParseYaml {
            path: receipt.path.clone(),
            source,
        })?;
    let normalized = document
        .get("refs")
        .and_then(|refs| refs.get("NAN_API_KEY"));
    let flat = document.get("NAN_API_KEY");
    if normalized.is_some() && flat.is_some() {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    let value = normalized
        .or(flat)
        .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
    let secret = value
        .as_str()
        .ok_or_else(|| ConfigurationError::ManagedDocumentChanged(receipt.path.clone()))?;
    let old = format!(
        "{}\nNAN_API_KEY: {}\n{}\n",
        receipt.begin,
        yaml_quote(secret)?,
        receipt.end
    );
    if sha256(old.as_bytes()) != receipt.block_sha256 {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    let path = if normalized.is_some() {
        vec!["refs".to_owned(), "NAN_API_KEY".to_owned()]
    } else {
        vec!["NAN_API_KEY".to_owned()]
    };
    Ok(Some(YamlReceipt {
        path: receipt.path.clone(),
        created_file: receipt.created_file,
        entries: vec![YamlEntryReceipt {
            path,
            value_sha256: hash_yaml(value)?,
            previous: None,
        }],
    }))
}

/// Normalize the native credential envelope before adding managed refs. Other
/// providers retain their values; only the NaN scalar receives an ownership receipt.
pub(super) fn normalize_deepseek_credentials(
    document: &mut YamlValue,
    plan: &YamlPlan,
    previous: Option<&YamlReceipt>,
) -> Result<Option<YamlReceipt>, ConfigurationError> {
    if plan
        .path
        .file_name()
        .is_none_or(|name| name != ".credentials.yaml")
        || plan
            .legacy_block
            .as_ref()
            .is_none_or(|block| block.begin != "# nan-harness:begin provider-credential")
    {
        return Ok(None);
    }
    let mapping = document
        .as_mapping_mut()
        .ok_or_else(|| ConfigurationError::YamlRootNotMapping(plan.path.clone()))?;
    if let Some(version) = mapping.get(YamlValue::String("version".to_owned())) {
        if version.as_u64() != Some(1)
            || mapping
                .get(YamlValue::String("refs".to_owned()))
                .is_some_and(|refs| !refs.is_mapping())
        {
            return Err(ConfigurationError::ManagedDocumentChanged(
                plan.path.clone(),
            ));
        }
        return Ok(None);
    }
    if mapping.contains_key(YamlValue::String("refs".to_owned())) {
        return Err(ConfigurationError::ManagedDocumentChanged(
            plan.path.clone(),
        ));
    }
    if mapping.is_empty() {
        return Ok(None);
    }
    let refs = std::mem::take(mapping);
    mapping.insert(
        YamlValue::String("version".to_owned()),
        YamlValue::Number(1.into()),
    );
    mapping.insert(
        YamlValue::String("refs".to_owned()),
        YamlValue::Mapping(refs),
    );
    Ok(previous.map(|receipt| {
        let mut normalized = receipt.clone();
        for entry in &mut normalized.entries {
            if entry.path == ["NAN_API_KEY"] {
                entry.path.insert(0, "refs".to_owned());
            }
        }
        normalized
    }))
}
