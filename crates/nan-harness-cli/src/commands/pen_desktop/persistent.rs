use super::PenDesktopError;
use super::documents::{
    hash_value, object_field_mut, patched_auth_document, patched_models_document, provider_entry,
    read_json_object, read_optional, serialize_document, sha256, write_json_private,
};
use super::paths::{PERSISTENT_SCHEMA_VERSION, PenPaths, PersistentEntryBackup, PersistentReceipt};
use super::session::ensure_no_pending_session;
use crate::commands::credentials;
use crate::commands::desktop::{
    DesktopSessionLock, create_private_directory, remove_file_if_present, write_private_atomic,
};
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::ResolvedConfig;
use serde_json::Value;
use std::fs;
use std::io::{BufRead as _, ErrorKind, Write as _};
use std::path::Path;

pub(super) fn persistent_configuration_exists() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    Ok(read_persistent_receipt(&paths)?.is_some())
}

pub(super) fn persistent_configuration_active() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    persistent_configuration_active_at(&paths)
}

pub(super) fn persistent_credential_is_current(
    saved_fingerprint: Option<&str>,
) -> Result<Option<bool>, PenDesktopError> {
    Ok(
        read_persistent_receipt(&PenPaths::from_environment()?)?.map(|receipt| {
            saved_fingerprint
                .is_some_and(|fingerprint| fingerprint == receipt.credential_fingerprint)
        }),
    )
}

pub(super) fn persistent_configuration_active_at(
    paths: &PenPaths,
) -> Result<bool, PenDesktopError> {
    let Some(receipt) = read_persistent_receipt(paths)? else {
        return Ok(false);
    };
    let models = read_json_object(&paths.models)?;
    let auth = read_json_object(&paths.auth)?;
    Ok(models
        .get("providers")
        .and_then(|providers| providers.get("nan"))
        .map(hash_value)
        .transpose()?
        .as_deref()
        == Some(&receipt.applied_models_sha256)
        && auth.get("nan").map(hash_value).transpose()?.as_deref()
            == Some(&receipt.applied_auth_sha256))
}

pub(super) async fn configure_persistent(
    refresh: bool,
    confirmed: bool,
    interactive: bool,
) -> Result<usize, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    ensure_no_pending_session(&paths)?;
    let previous_receipt = read_persistent_receipt(&paths)?;
    if refresh && previous_receipt.is_none() {
        return Err(PenDesktopError::PersistentNotConfigured);
    }
    if previous_receipt.is_some() && !persistent_configuration_active_at(&paths)? {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    if previous_receipt.is_none() && !confirmed && !confirm_persistent(interactive, &paths)? {
        return Err(PenDesktopError::ConfigurationCancelled);
    }
    let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
    apply_persistent_configuration(&paths, &config, &models, previous_receipt.as_ref())?;
    Ok(models.len())
}

pub(super) fn refresh_persistent_with_config(
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
) -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    ensure_no_pending_session(&paths)?;
    let Some(previous_receipt) = read_persistent_receipt(&paths)? else {
        return Ok(false);
    };
    if !persistent_configuration_active_at(&paths)? {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    apply_persistent_configuration(&paths, config, models, Some(&previous_receipt))?;
    Ok(true)
}

fn apply_persistent_configuration(
    paths: &PenPaths,
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
    previous_receipt: Option<&PersistentReceipt>,
) -> Result<(), PenDesktopError> {
    let models_root = read_json_object(&paths.models)?;
    let auth_root = read_json_object(&paths.auth)?;
    let models_file_existed =
        previous_receipt.map_or(paths.models.exists(), |receipt| receipt.models_file_existed);
    let auth_file_existed =
        previous_receipt.map_or(paths.auth.exists(), |receipt| receipt.auth_file_existed);
    let first_configuration = previous_receipt.is_none();
    let (models_backup, auth_backup) = if let Some(receipt) = previous_receipt {
        (receipt.models_backup.clone(), receipt.auth_backup.clone())
    } else {
        create_private_directory(&paths.persistent_backup_directory)?;
        let result = (|| {
            Ok::<_, PenDesktopError>((
                backup_persistent_entry(
                    paths,
                    "models-provider.json",
                    models_root
                        .get("providers")
                        .and_then(|providers| providers.get("nan")),
                )?,
                backup_persistent_entry(paths, "auth-entry.json", auth_root.get("nan"))?,
            ))
        })();
        match result {
            Ok(backups) => backups,
            Err(error) => {
                cleanup_uncommitted_persistent_backups(paths);
                return Err(error);
            }
        }
    };
    let models_document = patched_models_document(models_root, &config.provider_base_url, models)?;
    let auth_document = config
        .secrets
        .with_secret(&config.provider_credential_ref, |api_key| {
            patched_auth_document(auth_root, api_key)
        })
        .map_err(PenDesktopError::Secret)??;
    let models_entry = provider_entry(&models_document, super::PenDocumentKind::Models)?;
    let auth_entry = provider_entry(&auth_document, super::PenDocumentKind::Auth)?;
    let receipt = PersistentReceipt {
        schema_version: PERSISTENT_SCHEMA_VERSION,
        models_file_existed,
        auth_file_existed,
        models_backup,
        auth_backup,
        credential_fingerprint: credentials::credential_fingerprint(config)?,
        applied_models_sha256: hash_value(&models_entry)?,
        applied_auth_sha256: hash_value(&auth_entry)?,
        model_ids: models.iter().map(|model| model.id.clone()).collect(),
    };
    let old_models = read_optional(&paths.models)?;
    let old_auth = read_optional(&paths.auth)?;
    let old_receipt = read_optional(&paths.persistent_receipt)?;
    if let Err(error) = write_json_private(&paths.persistent_receipt, &receipt) {
        if first_configuration {
            cleanup_uncommitted_persistent_backups(paths);
        }
        return Err(error);
    }
    if let Err(error) = write_private_atomic(&paths.models, &models_document)
        .and_then(|()| write_private_atomic(&paths.auth, &auth_document))
    {
        let _ = restore_optional_file(&paths.models, old_models.as_deref());
        let _ = restore_optional_file(&paths.auth, old_auth.as_deref());
        let _ = restore_optional_file(&paths.persistent_receipt, old_receipt.as_deref());
        if first_configuration {
            cleanup_uncommitted_persistent_backups(paths);
        }
        return Err(PenDesktopError::State(error));
    }
    Ok(())
}

pub(super) fn backup_persistent_entry(
    paths: &PenPaths,
    backup_file: &str,
    value: Option<&Value>,
) -> Result<PersistentEntryBackup, PenDesktopError> {
    let contents = value
        .map(serde_json::to_vec)
        .transpose()
        .map_err(PenDesktopError::Serialize)?;
    if let Some(contents) = contents.as_deref() {
        write_private_atomic(
            &paths.persistent_backup_directory.join(backup_file),
            contents,
        )?;
    }
    Ok(PersistentEntryBackup {
        existed: contents.is_some(),
        sha256: contents.as_deref().map(sha256),
        backup_file: backup_file.to_owned(),
    })
}

fn read_persistent_backup(
    paths: &PenPaths,
    backup: &PersistentEntryBackup,
) -> Result<Option<Value>, PenDesktopError> {
    if !backup.existed {
        return Ok(None);
    }
    let contents = fs::read(paths.persistent_backup_directory.join(&backup.backup_file))
        .map_err(PenDesktopError::ReadBackup)?;
    if Some(sha256(&contents)) != backup.sha256 {
        return Err(PenDesktopError::BackupHashMismatch);
    }
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(PenDesktopError::ParseReceipt)
}

fn remove_persistent_backups(paths: &PenPaths) -> Result<(), PenDesktopError> {
    remove_file_if_present(
        &paths
            .persistent_backup_directory
            .join("models-provider.json"),
    )?;
    remove_file_if_present(&paths.persistent_backup_directory.join("auth-entry.json"))?;
    match fs::remove_dir(&paths.persistent_backup_directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PenDesktopError::RemoveBackup(error)),
    }
}

fn cleanup_uncommitted_persistent_backups(paths: &PenPaths) {
    let _ = remove_persistent_backups(paths);
}

pub(super) fn remove_persistent_configuration() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    remove_persistent_configuration_at(&paths)
}

pub(super) fn remove_persistent_configuration_at(
    paths: &PenPaths,
) -> Result<bool, PenDesktopError> {
    ensure_no_pending_session(paths)?;
    let Some(receipt) = read_persistent_receipt(paths)? else {
        return Ok(false);
    };
    let previous_models_provider = read_persistent_backup(paths, &receipt.models_backup)?;
    let previous_auth = read_persistent_backup(paths, &receipt.auth_backup)?;
    let models_state = persistent_entry_state(
        &paths.models,
        super::PenDocumentKind::Models,
        &receipt.applied_models_sha256,
        previous_models_provider.as_ref(),
    )?;
    let auth_state = persistent_entry_state(
        &paths.auth,
        super::PenDocumentKind::Auth,
        &receipt.applied_auth_sha256,
        previous_auth.as_ref(),
    )?;
    if models_state == PersistentEntryState::Changed || auth_state == PersistentEntryState::Changed
    {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    if models_state == PersistentEntryState::Applied {
        restore_persistent_entry(
            &paths.models,
            super::PenDocumentKind::Models,
            previous_models_provider.as_ref(),
            receipt.models_file_existed,
        )?;
    }
    if auth_state == PersistentEntryState::Applied {
        restore_persistent_entry(
            &paths.auth,
            super::PenDocumentKind::Auth,
            previous_auth.as_ref(),
            receipt.auth_file_existed,
        )?;
    }
    remove_persistent_backups(paths)?;
    remove_file_if_present(&paths.persistent_receipt)?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistentEntryState {
    Applied,
    Previous,
    Changed,
}

fn persistent_entry_state(
    path: &Path,
    kind: super::PenDocumentKind,
    applied_sha256: &str,
    previous: Option<&Value>,
) -> Result<PersistentEntryState, PenDesktopError> {
    let root = read_json_object(path)?;
    let current = match kind {
        super::PenDocumentKind::Models => root
            .get("providers")
            .and_then(|providers| providers.get("nan")),
        super::PenDocumentKind::Auth => root.get("nan"),
    };
    if current
        .map(hash_value)
        .transpose()?
        .is_some_and(|hash| hash == applied_sha256)
    {
        return Ok(PersistentEntryState::Applied);
    }
    let previous_matches = match (current, previous) {
        (None, None) => true,
        (Some(current), Some(previous)) => hash_value(current)? == hash_value(previous)?,
        _ => false,
    };
    Ok(if previous_matches {
        PersistentEntryState::Previous
    } else {
        PersistentEntryState::Changed
    })
}

pub(super) fn persistent_model_count() -> Result<Option<usize>, PenDesktopError> {
    persistent_model_count_at(&PenPaths::from_environment()?)
}

pub(super) fn persistent_model_count_at(
    paths: &PenPaths,
) -> Result<Option<usize>, PenDesktopError> {
    Ok(read_persistent_receipt(paths)?.map(|receipt| receipt.model_ids.len()))
}

fn read_persistent_receipt(paths: &PenPaths) -> Result<Option<PersistentReceipt>, PenDesktopError> {
    let Some(contents) = read_optional(&paths.persistent_receipt)? else {
        if paths.persistent_backup_directory.exists() {
            return Err(PenDesktopError::OrphanPersistentBackup);
        }
        return Ok(None);
    };
    let receipt: PersistentReceipt =
        serde_json::from_slice(&contents).map_err(PenDesktopError::ParseReceipt)?;
    if receipt.schema_version != PERSISTENT_SCHEMA_VERSION
        || receipt.models_backup.backup_file != "models-provider.json"
        || receipt.auth_backup.backup_file != "auth-entry.json"
    {
        return Err(PenDesktopError::InvalidReceipt);
    }
    let _ = read_persistent_backup(paths, &receipt.models_backup)?;
    let _ = read_persistent_backup(paths, &receipt.auth_backup)?;
    Ok(Some(receipt))
}

pub(super) fn restore_persistent_entry(
    path: &Path,
    kind: super::PenDocumentKind,
    previous: Option<&Value>,
    original_file_existed: bool,
) -> Result<(), PenDesktopError> {
    let mut root = read_json_object(path)?;
    match kind {
        super::PenDocumentKind::Models => {
            let providers = object_field_mut(&mut root, "providers", kind)?;
            match previous {
                Some(value) => {
                    providers.insert("nan".to_owned(), value.clone());
                }
                None => {
                    providers.remove("nan");
                }
            }
            if providers.is_empty() {
                root.remove("providers");
            }
        }
        super::PenDocumentKind::Auth => match previous {
            Some(value) => {
                root.insert("nan".to_owned(), value.clone());
            }
            None => {
                root.remove("nan");
            }
        },
    }
    if !original_file_existed && root.is_empty() {
        remove_file_if_present(path)?;
    } else {
        write_private_atomic(path, &serialize_document(&root)?)?;
    }
    Ok(())
}

fn restore_optional_file(path: &Path, contents: Option<&[u8]>) -> Result<(), PenDesktopError> {
    match contents {
        Some(contents) => write_private_atomic(path, contents)?,
        None => remove_file_if_present(path)?,
    }
    Ok(())
}

fn confirm_persistent(interactive: bool, paths: &PenPaths) -> Result<bool, PenDesktopError> {
    if !interactive {
        return Err(PenDesktopError::ConfirmationRequired);
    }
    eprintln!("nan-harness will add a persistent NaN provider to Pen Desktop.");
    eprintln!("The saved NaN API key will be copied into Pen's native credential file.");
    eprintln!("Managed files:");
    eprintln!("  - {}", paths.models.display());
    eprintln!("  - {}", paths.auth.display());
    let mut output = std::io::stderr().lock();
    write!(output, "Continue? [y/N] ").map_err(PenDesktopError::Prompt)?;
    output.flush().map_err(PenDesktopError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(PenDesktopError::Prompt)?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
