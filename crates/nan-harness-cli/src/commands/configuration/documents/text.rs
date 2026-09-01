use super::*;

pub(crate) fn prepare_text_block(
    plan: &TextBlockPlan,
    previous: Option<&TextBlockReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    validate_text_block_receipt(plan, previous)?;
    if plan.body.is_none() {
        return prepare_inactive_text_block(plan, previous);
    }
    let previous = previous.filter(|receipt| receipt.active);
    let TextBlockDocument {
        original,
        permissions,
        source,
    } = read_text_block_document(&plan.path)?;
    let desired = desired_text_block(plan);
    let replacement = prepare_text_block_replacement(plan, &source, previous, &desired)?;
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

pub(crate) fn validate_text_block_receipt(
    plan: &TextBlockPlan,
    previous: Option<&TextBlockReceipt>,
) -> Result<(), ConfigurationError> {
    if previous.is_some_and(|receipt| {
        receipt.path != plan.path || receipt.begin != plan.begin || receipt.end != plan.end
    }) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    Ok(())
}

pub(crate) struct TextBlockDocument {
    original: Option<Vec<u8>>,
    permissions: Option<Permissions>,
    source: String,
}

pub(crate) fn read_text_block_document(
    path: &Path,
) -> Result<TextBlockDocument, ConfigurationError> {
    let original = read_optional(path)?;
    let permissions = file_permissions(path)?;
    let source = match original.as_deref() {
        Some(contents) => String::from_utf8(contents.to_vec()).map_err(|source| {
            ConfigurationError::InvalidUtf8 {
                path: path.to_path_buf(),
                source,
            }
        })?,
        None => String::new(),
    };
    Ok(TextBlockDocument {
        original,
        permissions,
        source,
    })
}

pub(crate) fn desired_text_block(plan: &TextBlockPlan) -> String {
    format!(
        "{}\n{}\n{}\n",
        plan.begin,
        plan.body.as_deref().unwrap_or_default(),
        plan.end
    )
}

pub(crate) fn prepare_text_block_replacement(
    plan: &TextBlockPlan,
    source: &str,
    previous: Option<&TextBlockReceipt>,
    desired: &str,
) -> Result<String, ConfigurationError> {
    let Some(range) = block_range(source, &plan.begin, &plan.end)? else {
        return append_text_block(plan, source, previous, desired);
    };
    let receipt =
        previous.ok_or_else(|| ConfigurationError::UnmanagedDocumentConflict(plan.path.clone()))?;
    if sha256(source[range.clone()].as_bytes()) != receipt.block_sha256 {
        return Err(ConfigurationError::ManagedDocumentChanged(
            plan.path.clone(),
        ));
    }
    let mut rendered = source.to_owned();
    rendered.replace_range(range, desired);
    Ok(rendered)
}

pub(crate) fn append_text_block(
    plan: &TextBlockPlan,
    source: &str,
    previous: Option<&TextBlockReceipt>,
    desired: &str,
) -> Result<String, ConfigurationError> {
    if previous.is_some() {
        return Err(ConfigurationError::ManagedDocumentChanged(
            plan.path.clone(),
        ));
    }
    if plan.conflicting_keys.iter().any(|key| {
        source
            .lines()
            .any(|line| line.trim_start().starts_with(key))
    }) {
        return Err(ConfigurationError::UnmanagedDocumentConflict(
            plan.path.clone(),
        ));
    }
    let mut rendered = source.to_owned();
    if !rendered.is_empty() && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(desired);
    Ok(rendered)
}

pub(crate) fn prepare_inactive_text_block(
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

pub(crate) fn prepare_text_block_removal(
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
