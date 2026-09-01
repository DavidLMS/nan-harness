use super::*;

pub(crate) fn prepare_exact_file(
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

pub(crate) fn prepare_exact_file_removal(
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
