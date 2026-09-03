use crate::commands::desktop::write_private_atomic;
use crate::commands::zed_desktop::ZedDesktopError;
use crate::commands::zed_desktop::documents::{
    PatchedSettings, backup_file_name, read_optional, sha256,
};
use crate::commands::zed_desktop::paths::{SESSION_SCHEMA_VERSION, SessionReceipt};
use std::path::Path;

pub(super) fn from_prepared_settings(
    original: Option<&[u8]>,
    patched: &PatchedSettings,
) -> SessionReceipt {
    SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        file_existed: original.is_some(),
        original_sha256: original.map(sha256),
        backup_file: backup_file_name().to_owned(),
        applied_file_sha256: sha256(&patched.contents),
        applied_provider_sha256: patched.provider_sha256.clone(),
        applied_default_model_sha256: patched.default_model_sha256.clone(),
        created_language_models: patched.created_language_models,
        created_openai_compatible: patched.created_openai_compatible,
        created_agent: patched.created_agent,
        previous_default_model: patched.previous_default_model.clone(),
    }
}

pub(super) fn write(path: &Path, receipt: &SessionReceipt) -> Result<(), ZedDesktopError> {
    let mut payload = serde_json::to_vec_pretty(receipt).map_err(ZedDesktopError::Serialize)?;
    payload.push(b'\n');
    write_private_atomic(path, &payload)?;
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<Option<SessionReceipt>, ZedDesktopError> {
    let Some(contents) = read_optional(path)? else {
        return Ok(None);
    };
    let receipt = serde_json::from_slice(&contents).map_err(ZedDesktopError::ParseReceipt)?;
    validate(&receipt)?;
    Ok(Some(receipt))
}

fn validate(receipt: &SessionReceipt) -> Result<(), ZedDesktopError> {
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.backup_file != backup_file_name()
        || receipt.file_existed != receipt.original_sha256.is_some()
    {
        Err(ZedDesktopError::InvalidReceipt)
    } else {
        Ok(())
    }
}
