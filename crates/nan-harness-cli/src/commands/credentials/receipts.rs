use super::CredentialError;
use crate::commands::persistence::write_private_file;
use nan_harness_private_fs::{PrivateFileReadStatus, open_private_read};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

pub(super) const CREDENTIAL_RECEIPT_SCHEMA_VERSION: u8 = 1;

pub(super) const SAVED_KEY_REPAIR_WARNING: &str =
    "warning: restored private permissions on the saved NaN API key.";
pub(super) const CREDENTIAL_METADATA_REPAIR_WARNING: &str =
    "warning: restored private permissions on NaN credential metadata.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum StoredBackend {
    SystemKeyring,
    PrivateFile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CredentialReceipt {
    pub(super) schema_version: u8,
    pub(super) backend: StoredBackend,
}

pub(super) fn read_credential_receipt(
    path: &Path,
) -> Result<Option<CredentialReceipt>, CredentialError> {
    let Some(mut file) = open_private_file_for_read(path, CREDENTIAL_METADATA_REPAIR_WARNING)?
    else {
        return Ok(None);
    };
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|source| CredentialError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
    let receipt: CredentialReceipt =
        serde_json::from_slice(&contents).map_err(CredentialError::ParseReceipt)?;
    if receipt.schema_version != CREDENTIAL_RECEIPT_SCHEMA_VERSION {
        return Err(CredentialError::UnsupportedReceiptSchema(
            receipt.schema_version,
        ));
    }
    Ok(Some(receipt))
}

pub(super) fn write_credential_receipt(
    path: &Path,
    receipt: CredentialReceipt,
) -> Result<(), CredentialError> {
    let payload = serde_json::to_vec_pretty(&receipt).map_err(CredentialError::SerializeReceipt)?;
    write_private_file(path, &payload, None).map_err(CredentialError::State)
}

pub(super) fn open_private_file_for_read(
    path: &Path,
    repaired_warning: &'static str,
) -> Result<Option<File>, CredentialError> {
    match open_private_read(path) {
        Ok((file, status)) => {
            if let Some(warning) = private_file_repair_warning(status, repaired_warning) {
                eprintln!("{warning}");
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CredentialError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn private_file_repair_warning(
    status: PrivateFileReadStatus,
    repaired_warning: &'static str,
) -> Option<&'static str> {
    (status == PrivateFileReadStatus::Repaired).then_some(repaired_warning)
}

pub(super) fn remove_file_if_present(path: &Path) -> Result<(), CredentialError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CredentialError::RemoveFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}
