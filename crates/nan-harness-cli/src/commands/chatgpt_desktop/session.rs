//! Applying and restoring a managed `ChatGPT` Desktop session.
//!
//! Recovery invariants:
//! - the receipt is written before any managed write and removed last, so an
//!   interruption always leaves a session a later `--restore` can finish;
//! - the receipt records a durable restored phase before the backup it depends
//!   on is removed, and recovering that phase only finishes cleanup;
//! - nothing is deleted or overwritten while restoration cannot complete.
//!
//! Every file written here follows the private-file contract.

use super::profile::{ManagedProfile, validate_managed_profile};
use super::{
    CONFIG_BACKUP_NAME, CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME,
    SESSION_SCHEMA_VERSION, SESSION_SCHEMA_VERSION_2, SURFACE_ID,
};
use crate::commands::desktop::{reject_symlink, remove_file_if_present};
use backup::{capture_original_config, read_backup, sha256, write_private};
use nan_harness_runtime::RunningCodexDesktopBridge;
use serde::{Deserialize, Serialize};
use settings::{
    OwnedSetting, contains_managed_routing, managed_document, overlay_managed_settings,
    owned_settings, owned_settings_are_valid, restore_managed_settings,
};
use std::fs;
use std::str::FromStr;
use toml_edit::DocumentMut;

mod backup;
pub(super) mod settings;

const SHA256_LENGTH: usize = 64;

/// The receipt written by releases that always replaced the configuration.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionReceiptV1 {
    pub(super) schema_version: u8,
    pub(super) surface: String,
    pub(super) config_file: String,
    pub(super) model_catalog_file: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SessionPhase {
    Applied,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionReceiptV2 {
    pub(super) schema_version: u8,
    pub(super) surface: String,
    pub(super) config_file: String,
    pub(super) model_catalog_file: String,
    pub(super) phase: SessionPhase,
    pub(super) owned_settings: Vec<OwnedSetting>,
    pub(super) applied_config_sha256: String,
    pub(super) original_config: Option<OriginalConfigRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OriginalConfigRecord {
    pub(super) backup_file: String,
    pub(super) sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptSchema {
    schema_version: u8,
}

impl SessionReceiptV1 {
    fn is_valid(&self) -> bool {
        self.schema_version == SESSION_SCHEMA_VERSION
            && self.surface == SURFACE_ID
            && self.config_file == CONFIG_FILE_NAME
            && self.model_catalog_file == MODEL_CATALOG_FILE_NAME
    }
}

impl SessionReceiptV2 {
    fn new(managed: &DocumentMut, applied: &str, original: Option<&[u8]>) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION_2,
            surface: SURFACE_ID.to_owned(),
            config_file: CONFIG_FILE_NAME.to_owned(),
            model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
            phase: SessionPhase::Applied,
            owned_settings: owned_settings(managed),
            applied_config_sha256: sha256(applied.as_bytes()),
            original_config: original.map(|bytes| OriginalConfigRecord {
                backup_file: CONFIG_BACKUP_NAME.to_owned(),
                sha256: sha256(bytes),
            }),
        }
    }

    fn is_valid(&self) -> bool {
        self.schema_version == SESSION_SCHEMA_VERSION_2
            && self.surface == SURFACE_ID
            && self.config_file == CONFIG_FILE_NAME
            && self.model_catalog_file == MODEL_CATALOG_FILE_NAME
            && owned_settings_are_valid(&self.owned_settings)
            && is_sha256(&self.applied_config_sha256)
            && self
                .original_config
                .as_ref()
                .is_none_or(OriginalConfigRecord::is_valid)
    }

    fn restored(&self) -> Self {
        Self {
            phase: SessionPhase::Restored,
            ..self.clone()
        }
    }
}

impl OriginalConfigRecord {
    fn is_valid(&self) -> bool {
        self.backup_file == CONFIG_BACKUP_NAME && is_sha256(&self.sha256)
    }
}

fn is_sha256(digest: &str) -> bool {
    digest.len() == SHA256_LENGTH
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn apply_session(
    profile: &ManagedProfile,
    bridge: &RunningCodexDesktopBridge,
    web_search_enabled: bool,
) -> Result<(), ChatGptDesktopError> {
    let managed = managed_document(
        bridge.selected_model(),
        bridge.base_url(),
        &profile.catalog,
        web_search_enabled,
    );
    apply_managed_session(profile, &managed, bridge.model_catalog_json())
}

pub(super) fn apply_managed_session(
    profile: &ManagedProfile,
    managed: &DocumentMut,
    model_catalog_json: &str,
) -> Result<(), ChatGptDesktopError> {
    validate_managed_profile(profile)?;
    reject_orphaned_session_files(profile)?;
    let original = capture_original_config(profile)?;
    let mut document = match &original {
        Some(bytes) => parse_config(bytes)?,
        None => DocumentMut::new(),
    };
    overlay_managed_settings(&mut document, managed)?;
    let applied = document.to_string();
    let receipt = SessionReceiptV2::new(managed, &applied, original.as_deref());
    write_receipt(profile, &receipt)?;
    if let Some(bytes) = &original {
        write_private(&profile.config_backup, bytes)?;
    }
    write_private(&profile.catalog, model_catalog_json.as_bytes())?;
    write_private(&profile.config, applied.as_bytes())
}

/// Refuses a profile that still carries managed session state. Callers run this
/// once `restore_session` reported no receipt left to recover, so a native
/// configuration without nan-harness routing is legitimate and is adopted.
pub(super) fn reject_orphaned_session_files(
    profile: &ManagedProfile,
) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.config)?;
    if profile.catalog.exists() || profile.config_backup.exists() {
        return Err(ChatGptDesktopError::OrphanedSessionFiles);
    }
    let bytes = match fs::read(&profile.config) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    if contains_managed_routing(&parse_config(&bytes)?, &profile.catalog) {
        return Err(ChatGptDesktopError::OrphanedSessionFiles);
    }
    Ok(())
}

fn parse_config(bytes: &[u8]) -> Result<DocumentMut, ChatGptDesktopError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ChatGptDesktopError::MalformedConfig)?;
    DocumentMut::from_str(text).map_err(|_| ChatGptDesktopError::MalformedConfig)
}

fn write_receipt(
    profile: &ManagedProfile,
    receipt: &SessionReceiptV2,
) -> Result<(), ChatGptDesktopError> {
    let serialized =
        serde_json::to_vec_pretty(receipt).map_err(ChatGptDesktopError::SerializeState)?;
    write_private(&profile.receipt, &[serialized.as_slice(), b"\n"].concat())
}

pub(super) fn restore_session(profile: &ManagedProfile) -> Result<bool, ChatGptDesktopError> {
    reject_symlink(&profile.receipt)?;
    let contents = match fs::read(&profile.receipt) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    let schema: ReceiptSchema =
        serde_json::from_slice(&contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    match schema.schema_version {
        SESSION_SCHEMA_VERSION => restore_replaced_session(profile, &contents),
        SESSION_SCHEMA_VERSION_2 => restore_overlaid_session(profile, &contents),
        _ => Err(ChatGptDesktopError::InvalidReceipt),
    }?;
    Ok(true)
}

/// Recovers a version-1 session, whose configuration was entirely nan-harness's.
fn restore_replaced_session(
    profile: &ManagedProfile,
    contents: &[u8],
) -> Result<(), ChatGptDesktopError> {
    let receipt: SessionReceiptV1 =
        serde_json::from_slice(contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    if !receipt.is_valid() {
        return Err(ChatGptDesktopError::InvalidReceipt);
    }
    remove_file_if_present(&profile.config)?;
    remove_file_if_present(&profile.catalog)?;
    remove_file_if_present(&profile.receipt)?;
    Ok(())
}

fn restore_overlaid_session(
    profile: &ManagedProfile,
    contents: &[u8],
) -> Result<(), ChatGptDesktopError> {
    let receipt: SessionReceiptV2 =
        serde_json::from_slice(contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    if !receipt.is_valid() {
        return Err(ChatGptDesktopError::InvalidReceipt);
    }
    if receipt.phase == SessionPhase::Applied {
        restore_config(profile, &receipt)?;
        // The configuration no longer depends on the backup from here on.
        write_receipt(profile, &receipt.restored())?;
    }
    remove_file_if_present(&profile.catalog)?;
    remove_file_if_present(&profile.config_backup)?;
    remove_file_if_present(&profile.receipt)?;
    Ok(())
}

fn restore_config(
    profile: &ManagedProfile,
    receipt: &SessionReceiptV2,
) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.config)?;
    let current = match fs::read(&profile.config) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    let Some(current) = current else {
        return restore_original_config(profile, receipt.original_config.as_ref());
    };
    let digest = sha256(&current);
    if digest == receipt.applied_config_sha256 {
        return restore_original_config(profile, receipt.original_config.as_ref());
    }
    if receipt
        .original_config
        .as_ref()
        .is_some_and(|original| original.sha256 == digest)
    {
        // An interrupted apply never replaced the original bytes.
        return Ok(());
    }
    restore_changed_config(profile, receipt, &current)
}

fn restore_original_config(
    profile: &ManagedProfile,
    original: Option<&OriginalConfigRecord>,
) -> Result<(), ChatGptDesktopError> {
    let Some(original) = original else {
        return remove_file_if_present(&profile.config).map_err(ChatGptDesktopError::from);
    };
    let bytes = read_backup(profile, &original.sha256)?;
    write_private(&profile.config, &bytes)
}

/// Undoes the owned settings inside a configuration the app changed while it
/// ran, keeping every other setting it wrote.
fn restore_changed_config(
    profile: &ManagedProfile,
    receipt: &SessionReceiptV2,
    current: &[u8],
) -> Result<(), ChatGptDesktopError> {
    let mut document = parse_config(current)?;
    let original = match &receipt.original_config {
        Some(original) => Some(parse_config(&read_backup(profile, &original.sha256)?)?),
        None => None,
    };
    restore_managed_settings(&mut document, original.as_ref(), &receipt.owned_settings)?;
    if original.is_none() && document.as_table().is_empty() {
        return remove_file_if_present(&profile.config).map_err(ChatGptDesktopError::from);
    }
    write_private(&profile.config, document.to_string().as_bytes())
}

pub(super) fn selected_model_from_config(
    profile: &ManagedProfile,
    available: &[String],
) -> Option<String> {
    let contents = fs::read_to_string(&profile.config).ok()?;
    let document = DocumentMut::from_str(&contents).ok()?;
    let selected = document.get("model")?.as_str()?;
    available
        .iter()
        .find(|model| model.as_str() == selected)
        .cloned()
}
