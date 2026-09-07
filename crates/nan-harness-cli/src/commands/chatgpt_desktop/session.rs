//! Applying and restoring a managed `ChatGPT` Desktop session.
//!
//! A managed session overlays nan-harness routing onto the profile's native
//! `config.toml` instead of replacing it. Every session captures the original
//! state first, writes a private version-2 receipt that describes what it owns,
//! and only then writes the session files. Restoration undoes exactly those
//! owned settings: it rewrites the original bytes when the app left the applied
//! document untouched, and otherwise keeps every change the app made outside
//! the owned settings. Nothing is deleted while restoration is incomplete, so a
//! later `--restore` can always finish the work.

use super::profile::{ManagedProfile, validate_managed_profile};
use super::{
    CONFIG_BACKUP_NAME, CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME,
    SESSION_SCHEMA_VERSION, SESSION_SCHEMA_VERSION_2, SURFACE_ID,
};
use crate::commands::desktop::{reject_symlink, remove_file_if_present, write_private_atomic};
use backup::{
    OriginalConfig, capture_original_config, read_backup, sha256, write_backup, write_config,
};
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

/// The receipt of a session that overlays only the settings it owns.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionReceiptV2 {
    pub(super) schema_version: u8,
    pub(super) surface: String,
    pub(super) config_file: String,
    pub(super) model_catalog_file: String,
    /// The settings this session applied, and therefore the only ones it may
    /// remove or restore later.
    pub(super) owned_settings: Vec<OwnedSetting>,
    /// Digest of the exact document the session wrote, used to detect whether
    /// the app changed the configuration while it ran.
    pub(super) applied_config_sha256: String,
    /// The captured original configuration, absent when the profile had none.
    pub(super) original_config: Option<OriginalConfigRecord>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OriginalConfigRecord {
    /// Always the fixed managed backup name; a receipt may not redirect it.
    pub(super) backup_file: String,
    pub(super) sha256: String,
    pub(super) mode: Option<u32>,
}

/// The schema discriminator every receipt carries, read before any full parse.
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
    fn new(managed: &DocumentMut, applied: &str, original: Option<&OriginalConfig>) -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION_2,
            surface: SURFACE_ID.to_owned(),
            config_file: CONFIG_FILE_NAME.to_owned(),
            model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
            owned_settings: owned_settings(managed),
            applied_config_sha256: sha256(applied.as_bytes()),
            original_config: original.map(|original| OriginalConfigRecord {
                backup_file: CONFIG_BACKUP_NAME.to_owned(),
                sha256: original.sha256(),
                mode: original.mode,
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

/// Overlays the managed session onto the profile, capturing what it replaces.
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

/// Applies one rendered managed document and its model catalog to the profile.
pub(super) fn apply_managed_session(
    profile: &ManagedProfile,
    managed: &DocumentMut,
    model_catalog_json: &str,
) -> Result<(), ChatGptDesktopError> {
    validate_managed_profile(profile)?;
    reject_orphaned_session_files(profile)?;
    let original = capture_original_config(profile)?;
    let mut document = match &original {
        Some(original) => parse_config(&original.bytes)?,
        None => DocumentMut::new(),
    };
    overlay_managed_settings(&mut document, managed);
    let applied = document.to_string();
    // The receipt is written before any managed write, so an interrupted apply
    // always leaves a recoverable session rather than an unexplained overlay.
    let receipt = SessionReceiptV2::new(managed, &applied, original.as_ref());
    let serialized =
        serde_json::to_vec_pretty(&receipt).map_err(ChatGptDesktopError::SerializeState)?;
    write_private_atomic(&profile.receipt, &[serialized.as_slice(), b"\n"].concat())?;
    if let Some(original) = &original {
        write_backup(profile, original)?;
    }
    write_private_atomic(&profile.catalog, model_catalog_json.as_bytes())?;
    write_config(&profile.config, applied.as_bytes(), None)
}

/// Refuses to adopt a profile that still carries managed session state.
///
/// Callers run this once `restore_session` has reported that no receipt is left
/// to recover, so anything managed found here is a remnant of an interrupted
/// session. A native configuration that carries no nan-harness routing is
/// legitimate: it is preserved and the managed launch continues.
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

/// Restores an interrupted or finished session and reports whether one existed.
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

/// Recovers a version-2 session, undoing only the settings it owned.
fn restore_overlaid_session(
    profile: &ManagedProfile,
    contents: &[u8],
) -> Result<(), ChatGptDesktopError> {
    let receipt: SessionReceiptV2 =
        serde_json::from_slice(contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    if !receipt.is_valid() {
        return Err(ChatGptDesktopError::InvalidReceipt);
    }
    restore_config(profile, &receipt)?;
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
        // The app removed the configuration; the original still belongs there.
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
    write_config(&profile.config, &bytes, original.mode)
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
    restore_managed_settings(&mut document, original.as_ref(), &receipt.owned_settings);
    if original.is_none() && document.as_table().is_empty() {
        // The profile had no configuration and the app added no setting of its
        // own, so removing the managed overlay leaves nothing to keep.
        return remove_file_if_present(&profile.config).map_err(ChatGptDesktopError::from);
    }
    write_config(
        &profile.config,
        document.to_string().as_bytes(),
        receipt
            .original_config
            .as_ref()
            .and_then(|original| original.mode),
    )
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
