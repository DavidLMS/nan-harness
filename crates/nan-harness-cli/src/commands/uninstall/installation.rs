use super::UninstallError;
use crate::app::RecordInstallationArgs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const INSTALLATION_RECEIPT_SCHEMA_VERSION: u8 = 1;
const INSTALLATION_RECEIPT_FILE_NAME: &str = "installation.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationReceipt {
    schema_version: u8,
    executable_path: PathBuf,
    alias_path: PathBuf,
    user_path_entry_added: bool,
}

#[derive(Debug)]
pub(super) struct InstallationPaths {
    pub(super) executable_path: PathBuf,
    pub(super) alias_path: PathBuf,
    pub(super) remove_alias: bool,
    #[cfg(windows)]
    pub(super) user_path_entry_added: bool,
}

pub(super) fn record_installation(
    arguments: &RecordInstallationArgs,
    data_directory: &Path,
) -> Result<(), UninstallError> {
    validate_explicit_paths(&arguments.executable, &arguments.alias)?;
    if !alias_is_managed(&arguments.alias)? {
        return Err(UninstallError::UnsafeAliasPath(arguments.alias.clone()));
    }

    let current_executable = canonicalize_current_executable()?;
    let installed_executable = canonicalize_executable(&arguments.executable)?;
    if current_executable != installed_executable {
        return Err(UninstallError::ExecutableMismatch {
            expected: current_executable,
            actual: installed_executable,
        });
    }

    let user_path_entry_added = arguments.user_path_entry_added
        || previous_receipt(data_directory).is_some_and(|receipt| {
            receipt.executable_path == arguments.executable
                && receipt.alias_path == arguments.alias
                && receipt.user_path_entry_added
        });
    let receipt = InstallationReceipt {
        schema_version: INSTALLATION_RECEIPT_SCHEMA_VERSION,
        executable_path: arguments.executable.clone(),
        alias_path: arguments.alias.clone(),
        user_path_entry_added,
    };
    write_receipt(data_directory, &receipt)
}

pub(super) fn resolve_installation(
    data_directory: &Path,
) -> Result<InstallationPaths, UninstallError> {
    let current_executable = canonicalize_current_executable()?;
    let receipt_path = data_directory.join(INSTALLATION_RECEIPT_FILE_NAME);
    match fs::read(&receipt_path) {
        Ok(contents) => {
            let receipt: InstallationReceipt =
                serde_json::from_slice(&contents).map_err(UninstallError::ParseReceipt)?;
            if receipt.schema_version != INSTALLATION_RECEIPT_SCHEMA_VERSION {
                return Err(UninstallError::UnsupportedReceiptSchema(
                    receipt.schema_version,
                ));
            }
            validate_explicit_paths(&receipt.executable_path, &receipt.alias_path)?;
            let installed_executable = canonicalize_executable(&receipt.executable_path)?;
            if current_executable != installed_executable {
                return Err(UninstallError::ExecutableMismatch {
                    expected: current_executable,
                    actual: installed_executable,
                });
            }
            Ok(InstallationPaths {
                executable_path: receipt.executable_path,
                remove_alias: alias_is_managed(&receipt.alias_path)?,
                alias_path: receipt.alias_path,
                #[cfg(windows)]
                user_path_entry_added: receipt.user_path_entry_added,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(UninstallError::InstallationNotManaged)
        }
        Err(source) => Err(UninstallError::ReadReceipt {
            path: receipt_path,
            source,
        }),
    }
}

fn previous_receipt(data_directory: &Path) -> Option<InstallationReceipt> {
    let contents = fs::read(data_directory.join(INSTALLATION_RECEIPT_FILE_NAME)).ok()?;
    let receipt: InstallationReceipt = serde_json::from_slice(&contents).ok()?;
    (receipt.schema_version == INSTALLATION_RECEIPT_SCHEMA_VERSION).then_some(receipt)
}

fn validate_explicit_paths(executable: &Path, alias: &Path) -> Result<(), UninstallError> {
    if !executable.is_absolute() || !alias.is_absolute() {
        return Err(UninstallError::UnsafeInstallationPath(
            executable.to_path_buf(),
        ));
    }
    validate_executable_name(executable)?;
    if alias.file_name().and_then(|value| value.to_str()) != Some(alias_file_name())
        || executable.parent() != alias.parent()
    {
        return Err(UninstallError::UnsafeAliasPath(alias.to_path_buf()));
    }
    Ok(())
}

fn validate_executable_name(executable: &Path) -> Result<(), UninstallError> {
    let file_name = executable.file_name().and_then(|value| value.to_str());
    if file_name == Some(executable_file_name()) {
        Ok(())
    } else {
        Err(UninstallError::UnsafeInstallationPath(
            executable.to_path_buf(),
        ))
    }
}

fn canonicalize_current_executable() -> Result<PathBuf, UninstallError> {
    let path = env::current_exe().map_err(UninstallError::CurrentExecutable)?;
    canonicalize_executable(&path)
}

fn canonicalize_executable(path: &Path) -> Result<PathBuf, UninstallError> {
    fs::canonicalize(path).map_err(|source| UninstallError::CanonicalizeExecutable {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(windows))]
fn alias_is_managed(path: &Path) -> Result<bool, UninstallError> {
    if path.file_name().and_then(|value| value.to_str()) != Some(alias_file_name()) {
        return Ok(false);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(|target| target == Path::new(executable_file_name()))
            .map_err(|source| UninstallError::InspectAlias {
                path: path.to_path_buf(),
                source,
            }),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(UninstallError::InspectAlias {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(windows)]
fn alias_is_managed(path: &Path) -> Result<bool, UninstallError> {
    match fs::read(path) {
        Ok(contents) => {
            if path.file_name().and_then(|value| value.to_str()) != Some(alias_file_name()) {
                return Ok(false);
            }
            Ok(contents == b"@echo off\r\n\"%~dp0nan-harness.exe\" %*\r\n")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(UninstallError::InspectAlias {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_receipt(
    data_directory: &Path,
    receipt: &InstallationReceipt,
) -> Result<(), UninstallError> {
    fs::create_dir_all(data_directory).map_err(|source| UninstallError::CreateDataDirectory {
        path: data_directory.to_path_buf(),
        source,
    })?;
    let payload = serde_json::to_vec_pretty(receipt).map_err(UninstallError::SerializeReceipt)?;
    let mut temporary = tempfile::NamedTempFile::new_in(data_directory).map_err(|source| {
        UninstallError::WriteReceipt {
            path: data_directory.join(INSTALLATION_RECEIPT_FILE_NAME),
            source,
        }
    })?;
    temporary
        .write_all(&payload)
        .and_then(|()| temporary.flush())
        .map_err(|source| UninstallError::WriteReceipt {
            path: data_directory.join(INSTALLATION_RECEIPT_FILE_NAME),
            source,
        })?;
    let receipt_path = data_directory.join(INSTALLATION_RECEIPT_FILE_NAME);
    temporary
        .persist(&receipt_path)
        .map_err(|error| UninstallError::WriteReceipt {
            path: receipt_path,
            source: error.error,
        })?;
    Ok(())
}

#[cfg(windows)]
const fn executable_file_name() -> &'static str {
    "nan-harness.exe"
}

#[cfg(not(windows))]
const fn executable_file_name() -> &'static str {
    "nan-harness"
}

#[cfg(windows)]
const fn alias_file_name() -> &'static str {
    "nanh.cmd"
}

#[cfg(not(windows))]
const fn alias_file_name() -> &'static str {
    "nanh"
}
