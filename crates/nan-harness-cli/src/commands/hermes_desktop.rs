use crate::app::HermesDesktopArgs;
use crate::commands::credentials;
use crate::commands::install::check_required_runtime;
use crate::commands::persistence::{
    PersistenceManager, config_directory, discover_models, write_private_file,
};
use crate::error::CliError;
use crate::runner::discover_or_install_harness;
use nan_harness_adapters::{hermes_search_provider_files, render_hermes_desktop_provider_block};
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, HarnessKind,
};
use nan_harness_private_fs::{PrivatePathKind, open_private_new, restrict_path};
use nan_harness_runtime::{
    BridgeDiagnostic, ChatGatewayError, DesktopCompatibilityEvidence, DesktopCompatibilityStatus,
    DiscoveryReport, RunningChatCompletionsGateway, classify_desktop_version,
    desktop_compatibility, start_chat_completions_gateway,
};
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::process::{Child, Command as TokioCommand};

const PROFILE_NAME: &str = "nan";
const DIAGNOSTIC_PROFILE_PREFIX: &str = "nan-diagnostic-";
const PARKED_PROFILES_DIRECTORY: &str = ".nan-harness";
const RECOVERED_PROFILES_DIRECTORY: &str = "recovered";
const OWNERSHIP_SCHEMA_VERSION: u8 = 1;
const SESSION_SCHEMA_VERSION: u8 = 1;
const OWNER_MARKER_FILE: &str = ".nan-harness-owner.json";
const ENV_BLOCK_BEGIN: &str = "# nan-harness:begin hermes-desktop-session";
const ENV_BLOCK_END: &str = "# nan-harness:end hermes-desktop-session";
const DEFAULT_MODEL_ID: &str = "qwen3.6";
const UPDATE_WAIT_TIMEOUT: Duration = Duration::from_mins(20);
const UPDATE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const RELAUNCH_WAIT_TIMEOUT: Duration = Duration::from_mins(1);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DESKTOP_QUIESCENCE_INTERVAL: Duration = Duration::from_secs(5);

mod compatibility;
mod configuration;
mod diagnostics;
mod orchestration;
mod paths;
mod process;
mod profiles;
mod session;
#[cfg(test)]
mod tests;

use compatibility::*;
use configuration::*;
pub(crate) use diagnostics::HermesDesktopError;
use diagnostics::*;
use orchestration::*;
use paths::*;
use process::*;
use profiles::*;
use session::*;

pub(crate) async fn run(
    arguments: &HermesDesktopArgs,
    interactive: bool,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    validate_arguments(arguments)?;
    if arguments.no_chat_gateway && !arguments.run.dry_run && !arguments.restore {
        eprintln!(
            "warning: Chat Completions gateway disabled; provider usage and gateway-dependent search are unavailable"
        );
    }
    let paths = DesktopPaths::from_environment()?;

    if arguments.restore {
        return restore_command(&paths);
    }

    if arguments.run.dry_run {
        return print_dry_run(arguments, working_directory, &paths);
    }

    run_desktop_session(
        arguments,
        interactive,
        working_directory,
        bridge_diagnostics,
        &paths,
    )
    .await
}

pub(crate) fn persistent_profile_exists() -> Result<bool, HermesDesktopError> {
    let paths = DesktopPaths::from_environment()?;
    Ok(persistent_profile_exists_at(&paths))
}

pub(crate) fn remove_persistent_profile() -> Result<bool, HermesDesktopError> {
    let paths = DesktopPaths::from_environment()?;
    remove_persistent_profile_at(&paths, running_desktop)
}

fn persistent_profile_exists_at(paths: &DesktopPaths) -> bool {
    paths.ownership_receipt.exists()
        || paths.managed_profile.exists()
        || paths.parked_profile.exists()
}

fn remove_persistent_profile_at(
    paths: &DesktopPaths,
    running_desktop: impl FnOnce() -> Result<Option<DesktopProcess>, HermesDesktopError>,
) -> Result<bool, HermesDesktopError> {
    if !paths.session_receipt.exists() && !persistent_profile_exists_at(paths) {
        return Ok(false);
    }
    let _lock = SessionLock::acquire(paths)?;
    if paths.session_receipt.exists() {
        return Err(HermesDesktopError::PendingRecovery);
    }
    if !persistent_profile_exists_at(paths) {
        return Ok(false);
    }
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning);
    }
    park_managed_profile_if_owned(paths)?;
    let Some(ownership) = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)? else {
        return Ok(false);
    };
    let marker = read_optional_json::<OwnerMarker>(&paths.parked_profile.join(OWNER_MARKER_FILE))?
        .ok_or(HermesDesktopError::ManagedProfileMissing)?;
    validate_ownership(&ownership, &marker)?;
    fs::remove_dir_all(&paths.parked_profile).map_err(HermesDesktopError::RemoveProfile)?;
    remove_if_exists(&paths.ownership_receipt).map_err(HermesDesktopError::RemoveReceipt)?;
    remove_profile_guard(paths)?;
    reset_managed_active_profile(paths)?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerFingerprint {
    modified: Option<SystemTime>,
    length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopProcess {
    pid: u32,
    started: String,
}

#[derive(Debug)]
struct DesktopPaths {
    state_directory: PathBuf,
    lock: PathBuf,
    ownership_receipt: PathBuf,
    session_receipt: PathBuf,
    backup_directory: PathBuf,
    hermes_home: PathBuf,
    install_root: PathBuf,
    profiles_root: PathBuf,
    parked_profiles_root: PathBuf,
    recovered_profiles_root: PathBuf,
    managed_profile: PathBuf,
    parked_profile: PathBuf,
    active_profile: PathBuf,
    update_marker: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerMarker {
    schema_version: u8,
    owner_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnershipReceipt {
    schema_version: u8,
    owner_id: String,
    profile_name: String,
    gateway_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedProfileLocation {
    Active,
    Parked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfilePathKind {
    Missing,
    Directory,
    RegularFile,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum SessionMode {
    Persistent,
    Diagnostic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileBackup {
    existed: bool,
    original_sha256: Option<String>,
    backup_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReceipt {
    schema_version: u8,
    mode: SessionMode,
    profile: PathBuf,
    active_profile: FileBackup,
    environment: FileBackup,
    active_applied_sha256: String,
    environment_applied_sha256: String,
}
