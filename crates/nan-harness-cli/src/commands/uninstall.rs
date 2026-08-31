use crate::app::{RecordInstallationArgs, UninstallArgs};
use crate::commands::configuration::{ConfigurationError, ConfigurationManager};
use crate::commands::credentials::{CredentialError, CredentialManager};
use crate::commands::hermes_desktop::{self, HermesDesktopError};
use crate::commands::persistence::{
    PersistenceError, PersistenceManager, PersistentIntegration, RemovalOutcome,
};
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

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
struct InstallationPaths {
    executable_path: PathBuf,
    alias_path: PathBuf,
    remove_alias: bool,
    #[cfg(windows)]
    user_path_entry_added: bool,
}

pub(crate) fn run(arguments: &UninstallArgs, interactive: bool) -> Result<(), UninstallError> {
    let manager = PersistenceManager::from_environment()?;
    let data_directory = manager.state_directory().to_path_buf();
    validate_data_directory(&data_directory)?;
    ensure_no_pending_desktop_session(&data_directory)?;
    let installation = resolve_installation(&data_directory)?;
    let integrations = manager.configured_integrations()?;
    let configuration_manager = ConfigurationManager::from_environment()?;
    let native_configurations = configuration_manager.configured_harnesses()?;
    let credential_manager = CredentialManager::for_data_directory(&data_directory)?;
    let has_saved_credential = credential_manager.has_saved()?;
    let has_chatgpt_profile = data_directory.join("chatgpt-desktop/profile").exists();
    let has_hermes_profile = hermes_desktop::persistent_profile_exists()?;

    if !arguments.yes {
        if !interactive {
            return Err(UninstallError::ConfirmationRequired);
        }
        let confirmed = {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            prompt(
                &installation,
                &data_directory,
                &integrations,
                &native_configurations,
                has_saved_credential,
                has_chatgpt_profile,
                has_hermes_profile,
                &mut input,
                &mut output,
            )?
        };
        if !confirmed {
            println!("Uninstall cancelled.");
            return Ok(());
        }
    }

    if has_hermes_profile && hermes_desktop::remove_persistent_profile()? {
        println!("Hermes CLI/Desktop shared NaN profile removed.");
    }

    for (harness, outcome) in configuration_manager.remove_all()? {
        if outcome == RemovalOutcome::Removed {
            println!("NaN configuration removed from {harness}.");
        }
    }
    for integration in integrations {
        if manager.unpersist(integration)? == RemovalOutcome::Removed {
            println!("NaN provider removed from {integration}.");
        }
    }
    if credential_manager.remove_saved()? {
        println!("Saved NaN provider API key removed.");
    }

    if !installation.remove_alias && installation.alias_path.exists() {
        eprintln!(
            "warning: preserving '{}' because it is no longer managed by nan-harness",
            installation.alias_path.display()
        );
    }

    remove_installation(&installation, &data_directory)?;
    Ok(())
}

fn ensure_no_pending_desktop_session(data_directory: &Path) -> Result<(), UninstallError> {
    for (surface, relative) in [
        (
            "ChatGPT Desktop",
            "chatgpt-desktop/profile/.nan-session.json",
        ),
        ("Claude Desktop", "claude-desktop-receipt.json"),
        ("Hermes Desktop", "hermes-desktop/session.json"),
    ] {
        let receipt = data_directory.join(relative);
        match fs::symlink_metadata(&receipt) {
            Ok(_) => return Err(UninstallError::DesktopRecoveryRequired(surface)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UninstallError::InspectDataDirectory {
                    path: receipt,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn record_installation(
    arguments: &RecordInstallationArgs,
) -> Result<(), UninstallError> {
    let manager = PersistenceManager::from_environment()?;
    let data_directory = manager.state_directory();
    validate_data_directory(data_directory)?;
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

fn previous_receipt(data_directory: &Path) -> Option<InstallationReceipt> {
    let contents = fs::read(data_directory.join(INSTALLATION_RECEIPT_FILE_NAME)).ok()?;
    let receipt: InstallationReceipt = serde_json::from_slice(&contents).ok()?;
    (receipt.schema_version == INSTALLATION_RECEIPT_SCHEMA_VERSION).then_some(receipt)
}

fn resolve_installation(data_directory: &Path) -> Result<InstallationPaths, UninstallError> {
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
            resolve_legacy_installation(current_executable)
        }
        Err(source) => Err(UninstallError::ReadReceipt {
            path: receipt_path,
            source,
        }),
    }
}

fn resolve_legacy_installation(
    current_executable: PathBuf,
) -> Result<InstallationPaths, UninstallError> {
    validate_executable_name(&current_executable)?;
    let install_directory = current_executable
        .parent()
        .ok_or_else(|| UninstallError::UnsafeInstallationPath(current_executable.clone()))?;
    let alias_path = install_directory.join(alias_file_name_for_executable(&current_executable)?);
    if !alias_is_managed(&alias_path)? {
        return Err(UninstallError::InstallationNotManaged);
    }
    Ok(InstallationPaths {
        executable_path: current_executable,
        alias_path,
        remove_alias: true,
        #[cfg(windows)]
        user_path_entry_added: false,
    })
}

fn validate_explicit_paths(executable: &Path, alias: &Path) -> Result<(), UninstallError> {
    if !executable.is_absolute() || !alias.is_absolute() {
        return Err(UninstallError::UnsafeInstallationPath(
            executable.to_path_buf(),
        ));
    }
    validate_executable_name(executable)?;
    if alias.file_name().and_then(|value| value.to_str())
        != Some(alias_file_name_for_executable(executable)?)
        || executable.parent() != alias.parent()
    {
        return Err(UninstallError::UnsafeAliasPath(alias.to_path_buf()));
    }
    Ok(())
}

fn validate_executable_name(executable: &Path) -> Result<(), UninstallError> {
    let file_name = executable.file_name().and_then(|value| value.to_str());
    if file_name == Some(executable_file_name()) || file_name == Some(legacy_executable_file_name())
    {
        Ok(())
    } else {
        Err(UninstallError::UnsafeInstallationPath(
            executable.to_path_buf(),
        ))
    }
}

fn alias_file_name_for_executable(executable: &Path) -> Result<&'static str, UninstallError> {
    match executable.file_name().and_then(|value| value.to_str()) {
        Some(name) if name == executable_file_name() => Ok(alias_file_name()),
        Some(name) if name == legacy_executable_file_name() => Ok(legacy_alias_file_name()),
        _ => Err(UninstallError::UnsafeInstallationPath(
            executable.to_path_buf(),
        )),
    }
}

fn validate_data_directory(path: &Path) -> Result<(), UninstallError> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .parent()
            .is_some_and(|parent| parent.parent().is_none())
        || env::var_os(home_environment_variable()).is_some_and(|home| Path::new(&home) == path)
    {
        return Err(UninstallError::UnsafeDataDirectory(path.to_path_buf()));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(UninstallError::UnsafeDataDirectory(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::InspectDataDirectory {
            path: path.to_path_buf(),
            source,
        }),
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
    let expected_target = match path.file_name().and_then(|value| value.to_str()) {
        Some(name) if name == alias_file_name() => executable_file_name(),
        Some(name) if name == legacy_alias_file_name() => legacy_executable_file_name(),
        _ => return Ok(false),
    };
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::read_link(path)
            .map(|target| target == Path::new(expected_target))
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
            let expected = match path.file_name().and_then(|value| value.to_str()) {
                Some(name) if name == alias_file_name() => {
                    b"@echo off\r\n\"%~dp0nan-harness.exe\" %*\r\n".as_slice()
                }
                Some(name) if name == legacy_alias_file_name() => {
                    b"@echo off\r\n\"%~dp0nan.exe\" %*\r\n".as_slice()
                }
                _ => return Ok(false),
            };
            Ok(contents == expected)
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

#[allow(clippy::too_many_arguments)]
fn prompt(
    installation: &InstallationPaths,
    data_directory: &Path,
    integrations: &[PersistentIntegration],
    native_configurations: &[HarnessKind],
    has_saved_credential: bool,
    has_chatgpt_profile: bool,
    has_hermes_profile: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool, UninstallError> {
    writeln!(output, "\nnan-harness will remove:").map_err(UninstallError::Prompt)?;
    if integrations.is_empty() && native_configurations.is_empty() {
        writeln!(output, "  - Managed harness configurations: none")
            .map_err(UninstallError::Prompt)?;
    } else {
        let names = native_configurations
            .iter()
            .map(ToString::to_string)
            .chain(integrations.iter().map(ToString::to_string))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(output, "  - Managed harness configurations: {names}")
            .map_err(UninstallError::Prompt)?;
    }
    let credential = if has_saved_credential { "yes" } else { "none" };
    writeln!(output, "  - Saved NaN API key: {credential}").map_err(UninstallError::Prompt)?;
    if has_chatgpt_profile {
        writeln!(
            output,
            "  - ChatGPT Desktop profile: authentication, history, and cache"
        )
        .map_err(UninstallError::Prompt)?;
    }
    if has_hermes_profile {
        writeln!(
            output,
            "  - Hermes CLI/Desktop shared profile: conversations and local state"
        )
        .map_err(UninstallError::Prompt)?;
    }
    writeln!(
        output,
        "  - Application data: '{}'",
        data_directory.display()
    )
    .map_err(UninstallError::Prompt)?;
    writeln!(
        output,
        "  - Executable: '{}'",
        installation.executable_path.display()
    )
    .map_err(UninstallError::Prompt)?;
    if installation.remove_alias {
        writeln!(output, "  - Alias: '{}'", installation.alias_path.display())
            .map_err(UninstallError::Prompt)?;
    }
    write!(output, "\nContinue? [y/N]: ").map_err(UninstallError::Prompt)?;
    output.flush().map_err(UninstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(UninstallError::Prompt)?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(not(windows))]
fn remove_installation(
    installation: &InstallationPaths,
    data_directory: &Path,
) -> Result<(), UninstallError> {
    if installation.remove_alias {
        remove_file_if_present(&installation.alias_path)?;
    }
    remove_file_if_present(&installation.executable_path)?;
    remove_directory_if_present(data_directory)?;
    println!("nan-harness uninstalled successfully.");
    Ok(())
}

#[cfg(windows)]
fn remove_installation(
    installation: &InstallationPaths,
    data_directory: &Path,
) -> Result<(), UninstallError> {
    use std::process::Command;

    let mut helper = tempfile::Builder::new()
        .prefix("nan-uninstall-")
        .suffix(".ps1")
        .tempfile()
        .map_err(UninstallError::CreateHelper)?;
    helper
        .write_all(WINDOWS_UNINSTALL_HELPER.as_bytes())
        .and_then(|()| helper.flush())
        .map_err(UninstallError::CreateHelper)?;
    let (helper_file, helper_path) = helper
        .keep()
        .map_err(|error| UninstallError::CreateHelper(error.error))?;
    drop(helper_file);

    let alias_path = if installation.remove_alias {
        installation.alias_path.as_os_str()
    } else {
        std::ffi::OsStr::new("")
    };
    let install_directory = installation.executable_path.parent().ok_or_else(|| {
        UninstallError::UnsafeInstallationPath(installation.executable_path.clone())
    })?;
    let result = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&helper_path)
        .arg("-ParentProcessId")
        .arg(std::process::id().to_string())
        .arg("-ExecutablePath")
        .arg(&installation.executable_path)
        .arg("-AliasPath")
        .arg(alias_path)
        .arg("-DataDirectory")
        .arg(data_directory)
        .arg("-InstallDirectory")
        .arg(install_directory)
        .arg("-RemoveUserPath")
        .arg(installation.user_path_entry_added.to_string())
        .spawn();
    if let Err(source) = result {
        let _ = fs::remove_file(&helper_path);
        return Err(UninstallError::StartHelper(source));
    }
    println!("nan-harness uninstall scheduled; cleanup will finish after this process exits.");
    Ok(())
}

#[cfg(windows)]
const WINDOWS_UNINSTALL_HELPER: &str = r#"param(
    [Parameter(Mandatory = $true)][int]$ParentProcessId,
    [Parameter(Mandatory = $true)][string]$ExecutablePath,
    [Parameter(Mandatory = $true)][AllowEmptyString()][string]$AliasPath,
    [Parameter(Mandatory = $true)][string]$DataDirectory,
    [Parameter(Mandatory = $true)][string]$InstallDirectory,
    [Parameter(Mandatory = $true)][string]$RemoveUserPath
)
$ErrorActionPreference = "Stop"
$scriptPath = $MyInvocation.MyCommand.Path
try {
    Wait-Process -Id $ParentProcessId -ErrorAction SilentlyContinue
    if ($AliasPath -and (Test-Path -LiteralPath $AliasPath)) {
        Remove-Item -LiteralPath $AliasPath -Force
    }
    if (Test-Path -LiteralPath $ExecutablePath) {
        Remove-Item -LiteralPath $ExecutablePath -Force
    }
    if (Test-Path -LiteralPath $DataDirectory) {
        Remove-Item -LiteralPath $DataDirectory -Recurse -Force
    }
    if ($RemoveUserPath -eq "true") {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = @($userPath -split ';' | Where-Object {
            $_ -and $_.TrimEnd('\') -ine $InstallDirectory.TrimEnd('\')
        })
        [Environment]::SetEnvironmentVariable("Path", ($entries -join ';'), "User")
    }
    Write-Host "nan-harness uninstalled successfully."
} catch {
    Write-Error "nan-harness uninstall cleanup failed: $_"
} finally {
    Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue
}
"#;

#[cfg(not(windows))]
fn remove_file_if_present(path: &Path) -> Result<(), UninstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::RemoveFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(not(windows))]
fn remove_directory_if_present(path: &Path) -> Result<(), UninstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::RemoveDataDirectory {
            path: path.to_path_buf(),
            source,
        }),
    }
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
const fn legacy_executable_file_name() -> &'static str {
    "nan.exe"
}

#[cfg(not(windows))]
const fn legacy_executable_file_name() -> &'static str {
    "nan"
}

#[cfg(windows)]
const fn alias_file_name() -> &'static str {
    "nan.cmd"
}

#[cfg(not(windows))]
const fn alias_file_name() -> &'static str {
    "nan"
}

#[cfg(windows)]
const fn legacy_alias_file_name() -> &'static str {
    "nan-harness.cmd"
}

#[cfg(not(windows))]
const fn legacy_alias_file_name() -> &'static str {
    "nan-harness"
}

#[cfg(windows)]
const fn home_environment_variable() -> &'static str {
    "USERPROFILE"
}

#[cfg(not(windows))]
const fn home_environment_variable() -> &'static str {
    "HOME"
}

#[derive(Debug, Error)]
pub(crate) enum UninstallError {
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    HermesDesktop(#[from] HermesDesktopError),
    #[error("uninstall confirmation requires an interactive terminal; rerun with --yes")]
    ConfirmationRequired,
    #[error(
        "{0} has recovery state; close the app and run its `nan ...-desktop --restore` command before uninstalling"
    )]
    DesktopRecoveryRequired(&'static str),
    #[error("this nan-harness executable is not managed by the release installer")]
    InstallationNotManaged,
    #[error("could not determine the current nan-harness executable: {0}")]
    CurrentExecutable(std::io::Error),
    #[error("could not resolve executable '{}': {source}", path.display())]
    CanonicalizeExecutable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("installation receipt points to '{}', but the running executable is '{}'", actual.display(), expected.display())]
    ExecutableMismatch { expected: PathBuf, actual: PathBuf },
    #[error("unsafe nan-harness installation path '{}'", .0.display())]
    UnsafeInstallationPath(PathBuf),
    #[error("unsafe nan-harness alias path '{}'", .0.display())]
    UnsafeAliasPath(PathBuf),
    #[error("unsafe nan-harness application data directory '{}'", .0.display())]
    UnsafeDataDirectory(PathBuf),
    #[error("could not inspect application data directory '{}': {source}", path.display())]
    InspectDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not inspect alias '{}': {source}", path.display())]
    InspectAlias {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read installation receipt '{}': {source}", path.display())]
    ReadReceipt {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("installation receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("installation receipt uses unsupported schema version {0}")]
    UnsupportedReceiptSchema(u8),
    #[error("could not create application data directory '{}': {source}", path.display())]
    CreateDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize the installation receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("could not write installation receipt '{}': {source}", path.display())]
    WriteReceipt {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read uninstall confirmation: {0}")]
    Prompt(std::io::Error),
    #[cfg(not(windows))]
    #[error("could not remove '{}': {source}", path.display())]
    RemoveFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(not(windows))]
    #[error("could not remove application data '{}': {source}", path.display())]
    RemoveDataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(windows)]
    #[error("could not create the Windows uninstall helper: {0}")]
    CreateHelper(std::io::Error),
    #[cfg(windows)]
    #[error("could not start the Windows uninstall helper: {0}")]
    StartHelper(std::io::Error),
}

impl UninstallError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Persistence(error) => error.code(),
            Self::Configuration(error) => error.code(),
            Self::Credential(error) => error.code(),
            Self::HermesDesktop(error) => error.code(),
            Self::ConfirmationRequired | Self::DesktopRecoveryRequired(_) | Self::Prompt(_) => {
                "NH-UNINSTALL-001"
            }
            Self::InstallationNotManaged
            | Self::ExecutableMismatch { .. }
            | Self::UnsafeInstallationPath(_)
            | Self::UnsafeAliasPath(_)
            | Self::UnsafeDataDirectory(_) => "NH-UNINSTALL-002",
            Self::ReadReceipt { .. }
            | Self::ParseReceipt(_)
            | Self::UnsupportedReceiptSchema(_)
            | Self::SerializeReceipt(_)
            | Self::WriteReceipt { .. } => "NH-UNINSTALL-003",
            Self::CurrentExecutable(_)
            | Self::CanonicalizeExecutable { .. }
            | Self::InspectDataDirectory { .. }
            | Self::InspectAlias { .. }
            | Self::CreateDataDirectory { .. } => "NH-UNINSTALL-004",
            #[cfg(not(windows))]
            Self::RemoveFile { .. } | Self::RemoveDataDirectory { .. } => "NH-UNINSTALL-004",
            #[cfg(windows)]
            Self::CreateHelper(_) | Self::StartHelper(_) => "NH-UNINSTALL-005",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallationPaths, prompt};
    use crate::commands::persistence::PersistentIntegration;
    use std::io::Cursor;
    use std::path::PathBuf;

    #[test]
    fn prompt_defaults_to_preserving_the_installation() {
        for response in ["", "\n", "n\n", "anything\n"] {
            let mut input = Cursor::new(response.as_bytes());
            let mut output = Vec::new();
            assert!(
                !prompt(
                    &installation(),
                    std::path::Path::new("/tmp/state"),
                    &[PersistentIntegration::Pi, PersistentIntegration::Aider],
                    &[],
                    true,
                    false,
                    false,
                    &mut input,
                    &mut output,
                )
                .expect("prompt should complete")
            );
        }
    }

    #[test]
    fn prompt_accepts_only_explicit_confirmation() {
        for response in ["y\n", "Y\n", "yes\n", "YES\n"] {
            let mut input = Cursor::new(response.as_bytes());
            let mut output = Vec::new();
            assert!(
                prompt(
                    &installation(),
                    std::path::Path::new("/tmp/state"),
                    &[PersistentIntegration::Pi],
                    &[],
                    false,
                    false,
                    false,
                    &mut input,
                    &mut output,
                )
                .expect("prompt should complete")
            );
        }
    }

    fn installation() -> InstallationPaths {
        InstallationPaths {
            executable_path: PathBuf::from("/tmp/bin/nan-harness"),
            alias_path: PathBuf::from("/tmp/bin/nan"),
            remove_alias: true,
            #[cfg(windows)]
            user_path_entry_added: false,
        }
    }
}
