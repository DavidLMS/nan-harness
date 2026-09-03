use crate::app::ChatGptDesktopArgs;
use crate::commands::desktop::{DesktopSessionLock, DesktopStateError};
use crate::commands::persistence::{PersistenceError, PersistenceManager};
use crate::error::CliError;
use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy};
use nan_harness_runtime::{
    BridgeDiagnostic, CodexDesktopBridgeError, DesktopCompatibilityError,
    evaluate_desktop_compatibility,
};
use semver::Version;
use thiserror::Error;

const SURFACE_ID: &str = "chatgpt-desktop";
const STATE_DIRECTORY_NAME: &str = "chatgpt-desktop";
const PROFILE_DIRECTORY_NAME: &str = "profile";
const PROFILE_MARKER_NAME: &str = ".nan-managed-profile.json";
const SESSION_RECEIPT_NAME: &str = ".nan-session.json";
const CONFIG_FILE_NAME: &str = "config.toml";
const MODEL_CATALOG_FILE_NAME: &str = "nan-model-catalog.json";
const SESSION_TOKEN_ENVIRONMENT: &str = "NAN_HARNESS_SESSION_TOKEN";
const PROFILE_SCHEMA_VERSION: u8 = 1;
const SESSION_SCHEMA_VERSION: u8 = 1;
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);
const BRIDGE_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

mod installation;
mod orchestration;
mod platform;
mod process;
mod profile;
mod session;
#[cfg(test)]
mod tests;

use installation::discover_installation;
use orchestration::run_managed_session;
use platform::chatgpt_is_running;
use process::require_app_stopped;
use profile::{ManagedProfile, validate_managed_profile};
use session::{reject_orphaned_session_files, restore_session};

pub(crate) async fn run(
    arguments: &ChatGptDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    if arguments.dry_run {
        let mut plan = DesktopLaunchPlan::new(
            DesktopHarnessKind::ChatGpt,
            DesktopTransport::ResponsesBridge,
        );
        plan.executable.clone_from(&arguments.executable);
        plan.selected_model.clone_from(&arguments.model);
        plan.auxiliary_model.clone_from(&arguments.aux_model);
        plan.persistent_profile = true;
        plan.private_diagnostics = arguments.debug;
        plan.web_search_policy = if arguments.search.no_search {
            WebSearchPolicy::Disabled
        } else if arguments.search.force_search {
            WebSearchPolicy::Force
        } else {
            WebSearchPolicy::Auto
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(ChatGptDesktopError::SerializeState)?
        );
        return Ok(0);
    }

    let manager = PersistenceManager::from_environment()?;
    let state_directory = manager.state_directory().join(STATE_DIRECTORY_NAME);
    if arguments.restore {
        let _lock =
            DesktopSessionLock::acquire(&state_directory).map_err(ChatGptDesktopError::from)?;
        require_app_stopped()?;
        let profile = ManagedProfile::for_manager(&manager);
        if !profile.root.exists() {
            println!("No managed ChatGPT Desktop session needs recovery.");
            return Ok(0);
        }
        validate_managed_profile(&profile)?;
        if restore_session(&profile)? {
            println!("Recovered the managed ChatGPT Desktop profile.");
        } else {
            reject_orphaned_session_files(&profile)?;
            println!("No managed ChatGPT Desktop session needs recovery.");
        }
        return Ok(0);
    }

    let installation = discover_installation(arguments.executable.as_deref())?;
    let compatibility = evaluate_desktop_compatibility(
        &installation.app_version,
        &installation.bundled_codex_version,
    )
    .map_err(ChatGptDesktopError::from)?;
    orchestration::enforce_compatibility(
        &compatibility,
        arguments.allow_unsupported,
        arguments.allow_untested,
    )?;
    let remembered_model = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::ChatGpt)
            .ok()
            .flatten()
            .map(|selection| selection.model)
    } else {
        None
    };
    if chatgpt_is_running()? {
        return Err(ChatGptDesktopError::AppAlreadyRunning.into());
    }

    run_managed_session(
        arguments,
        interactive,
        bridge_diagnostics,
        &manager,
        &state_directory,
        &installation,
        remembered_model.as_deref(),
    )
    .await
}

#[derive(Debug, Error)]
pub(crate) enum ChatGptDesktopError {
    #[error(
        "ChatGPT Desktop Preview is not available on this platform; it supports the official macOS, Windows, and Linux apps"
    )]
    #[cfg_attr(
        any(target_os = "macos", target_os = "windows", target_os = "linux"),
        allow(dead_code)
    )]
    UnsupportedPlatform,
    #[error("the official ChatGPT Desktop app was not found in a supported installation directory")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    AppNotFound,
    #[error("the ChatGPT Desktop installation is incomplete or invalid")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    InvalidInstallation,
    #[error("could not run a ChatGPT Desktop version command: {0}")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    VersionCommand(std::io::Error),
    #[error("a ChatGPT Desktop version command failed")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    VersionCommandFailed,
    #[error("could not parse the ChatGPT Desktop or bundled Codex version")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    UnparseableVersion,
    #[error(transparent)]
    Compatibility(#[from] DesktopCompatibilityError),
    #[error(
        "this ChatGPT Desktop release is older than the supported minimum (app {minimum_app}, bundled Codex {minimum_codex})"
    )]
    OlderUnsupported {
        minimum_app: Version,
        minimum_codex: Version,
    },
    #[error(
        "this ChatGPT Desktop release is newer than the tested range (app {last_app}, bundled Codex {last_codex}); rerun with --allow-untested to try it"
    )]
    NewerUntested {
        last_app: Version,
        last_codex: Version,
    },
    #[error("ChatGPT is already running; quit it completely and try again")]
    AppAlreadyRunning,
    #[error("another ChatGPT instance won the startup race; quit ChatGPT completely and try again")]
    SingletonRace,
    #[error(
        "ChatGPT Desktop did not terminate, so its launch-scoped configuration was preserved; quit it completely, then run `nanh chatgpt-desktop --restore`"
    )]
    AppDidNotTerminate,
    #[error("ChatGPT Desktop exited before its managed session became ready")]
    AppExitedDuringStartup,
    #[error("could not inspect the running ChatGPT process: {0}")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    InspectProcess(std::io::Error),
    #[error("the operating system could not determine whether ChatGPT is running")]
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
        allow(dead_code)
    )]
    ProcessInspectionFailed,
    #[error(transparent)]
    State(#[from] DesktopStateError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("the existing ChatGPT Desktop profile is not owned by nan-harness")]
    UnmanagedProfile,
    #[error("the ChatGPT Desktop profile ownership marker is invalid")]
    InvalidMarker,
    #[error("the ChatGPT Desktop recovery receipt is invalid")]
    InvalidReceipt,
    #[error(
        "managed Desktop configuration exists without a valid recovery receipt; preserve the profile and run nanh chatgpt-desktop --restore after inspecting it"
    )]
    OrphanedSessionFiles,
    #[error("could not inspect the managed ChatGPT Desktop profile: {0}")]
    InspectProfile(std::io::Error),
    #[error("could not read managed ChatGPT Desktop state: {0}")]
    ReadState(std::io::Error),
    #[error("could not write managed ChatGPT Desktop state: {0}")]
    WriteState(std::io::Error),
    #[error("the ChatGPT Desktop profile marker is not valid JSON: {0}")]
    ParseMarker(serde_json::Error),
    #[error("the ChatGPT Desktop recovery receipt is not valid JSON: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("could not serialize managed ChatGPT Desktop state: {0}")]
    SerializeState(serde_json::Error),
    #[error(transparent)]
    Bridge(#[from] CodexDesktopBridgeError),
    #[error("the ChatGPT Desktop bridge stopped unexpectedly")]
    BridgeExited,
    #[error("ChatGPT Desktop did not authenticate to its isolated bridge in time")]
    BridgeHandshakeTimeout,
    #[error("could not start ChatGPT Desktop: {0}")]
    StartApp(std::io::Error),
    #[error("could not wait for ChatGPT Desktop: {0}")]
    WaitForApp(std::io::Error),
    #[error("could not stop ChatGPT Desktop: {0}")]
    StopApp(std::io::Error),
}

impl ChatGptDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform | Self::AppNotFound | Self::InvalidInstallation => {
                "NH-DESKTOP-004"
            }
            Self::VersionCommand(_)
            | Self::VersionCommandFailed
            | Self::UnparseableVersion
            | Self::Compatibility(_)
            | Self::OlderUnsupported { .. }
            | Self::NewerUntested { .. } => "NH-DESKTOP-005",
            Self::AppAlreadyRunning
            | Self::SingletonRace
            | Self::AppDidNotTerminate
            | Self::InspectProcess(_)
            | Self::ProcessInspectionFailed => "NH-DESKTOP-006",
            Self::State(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::UnmanagedProfile
            | Self::InvalidMarker
            | Self::InvalidReceipt
            | Self::OrphanedSessionFiles
            | Self::ParseMarker(_)
            | Self::ParseReceipt(_) => "NH-DESKTOP-007",
            Self::InspectProfile(_)
            | Self::ReadState(_)
            | Self::WriteState(_)
            | Self::SerializeState(_) => "NH-DESKTOP-008",
            Self::Bridge(error) => error.code(),
            Self::BridgeExited | Self::BridgeHandshakeTimeout => "NH-DESKTOP-009",
            Self::AppExitedDuringStartup
            | Self::StartApp(_)
            | Self::WaitForApp(_)
            | Self::StopApp(_) => "NH-DESKTOP-010",
        }
    }
}
