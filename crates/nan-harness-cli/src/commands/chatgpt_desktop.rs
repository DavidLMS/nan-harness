use crate::app::ChatGptDesktopArgs;
use crate::commands::desktop::{
    DesktopSessionLock, DesktopStateError, create_private_directory, create_private_new,
    reject_symlink, remove_file_if_present, write_private_atomic,
};
use crate::commands::persistence::{PersistenceError, PersistenceManager};
use crate::error::CliError;
use crate::runner::install_signal_handlers;
use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy};
use nan_harness_runtime::{
    BridgeDiagnostic, CodexDesktopBridgeError, DesktopCompatibilityError,
    DesktopCompatibilityReport, DesktopCompatibilityStatus, RunningCodexDesktopBridge,
    evaluate_desktop_compatibility, start_codex_desktop_bridge,
};
use semver::Version;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tokio::process::{Child, Command};

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
#[cfg(target_os = "macos")]
const APP_BUNDLE_ID: &str = "com.openai.codex";
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
const BRIDGE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
struct ChatGptInstallation {
    executable: PathBuf,
    app_version: Version,
    bundled_codex_version: Version,
}

#[derive(Debug, Clone)]
struct ManagedProfile {
    root: PathBuf,
    marker: PathBuf,
    receipt: PathBuf,
    config: PathBuf,
    catalog: PathBuf,
}

impl ManagedProfile {
    fn for_manager(manager: &PersistenceManager) -> Self {
        let root = manager
            .state_directory()
            .join(STATE_DIRECTORY_NAME)
            .join(PROFILE_DIRECTORY_NAME);
        Self {
            marker: root.join(PROFILE_MARKER_NAME),
            receipt: root.join(SESSION_RECEIPT_NAME),
            config: root.join(CONFIG_FILE_NAME),
            catalog: root.join(MODEL_CATALOG_FILE_NAME),
            root,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileMarker {
    schema_version: u8,
    surface: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReceipt {
    schema_version: u8,
    surface: String,
    config_file: String,
    model_catalog_file: String,
}

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
    enforce_compatibility(
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
    let app_running = chatgpt_is_running()?;
    if app_running {
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

async fn run_managed_session(
    arguments: &ChatGptDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
    manager: &PersistenceManager,
    state_directory: &Path,
    installation: &ChatGptInstallation,
    remembered_model: Option<&str>,
) -> Result<i32, CliError> {
    let _lock = DesktopSessionLock::acquire(state_directory).map_err(ChatGptDesktopError::from)?;
    let profile = ManagedProfile::for_manager(manager);
    ensure_managed_profile(&profile)?;
    if restore_session(&profile)? {
        eprintln!("Recovered configuration from an interrupted ChatGPT Desktop session.");
    }
    reject_orphaned_session_files(&profile)?;

    let mut config = crate::commands::credentials::resolve_or_onboard(
        arguments.provider_base_url.clone(),
        interactive,
    )
    .await?;
    let discovered_models = config.model_catalog.take();
    let mut bridge = start_codex_desktop_bridge(
        &config.config,
        discovered_models,
        arguments.model.as_deref().or(remembered_model),
        arguments.aux_model.as_deref(),
        !arguments.search.no_search,
    )
    .await
    .map_err(ChatGptDesktopError::from)?;
    apply_session(&profile, &bridge, !arguments.search.no_search)?;
    if arguments.debug {
        eprintln!(
            "warning: debug mode exposes verbose ChatGPT Desktop logs; treat terminal output as private"
        );
    }
    eprintln!(
        "Starting ChatGPT Desktop Preview with NaN model '{}'.",
        bridge.selected_model()
    );
    if bridge.auxiliary_model() != bridge.selected_model() {
        eprintln!(
            "Desktop background requests use auxiliary NaN model '{}'.",
            bridge.auxiliary_model()
        );
    }

    let cancellation = nan_harness_runtime::CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let result = supervise_desktop(
        installation,
        &profile,
        &mut bridge,
        arguments.debug,
        &cancellation,
        bridge_diagnostics,
    )
    .await;
    signal_task.abort();
    let selected_after_exit = selected_model_from_config(&profile, bridge.available_models());
    if chatgpt_is_running()? {
        bridge.shutdown();
        let _ = bridge.wait().await;
        return Err(ChatGptDesktopError::AppDidNotTerminate.into());
    }
    bridge.shutdown();
    let bridge_wait = bridge.wait().await;
    let usage = bridge.usage();
    let cleanup = restore_session(&profile);
    cleanup?;
    let exit_code = result?;
    bridge_wait
        .map_err(CodexDesktopBridgeError::from)
        .map_err(ChatGptDesktopError::from)?;
    if let Some(model) = selected_after_exit
        && let Err(error) = manager.save_last_desktop_selection(DesktopHarnessKind::ChatGpt, &model)
    {
        eprintln!("warning: could not save the last Desktop model: {error}");
    }
    let outcome = if exit_code == 0 {
        nan_harness_runtime::ExecutionOutcome::Succeeded
    } else {
        nan_harness_runtime::ExecutionOutcome::Failed
    };
    if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
        eprintln!("{summary}");
    }
    Ok(exit_code)
}

fn enforce_compatibility(
    report: &DesktopCompatibilityReport,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), ChatGptDesktopError> {
    match report.status {
        DesktopCompatibilityStatus::Tested => Ok(()),
        DesktopCompatibilityStatus::ContractOnly => {
            eprintln!(
                "warning: ChatGPT Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested if allow_untested => {
            eprintln!(
                "warning: this ChatGPT Desktop version is newer than the pinned compatibility evidence"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested => Err(ChatGptDesktopError::NewerUntested {
            last_app: report.last_compatible_app_version.clone(),
            last_codex: report.last_compatible_bundled_codex_version.clone(),
        }),
        DesktopCompatibilityStatus::OlderUnsupported if allow_unsupported => {
            eprintln!("warning: running an older unsupported ChatGPT Desktop version");
            Ok(())
        }
        DesktopCompatibilityStatus::OlderUnsupported => {
            Err(ChatGptDesktopError::OlderUnsupported {
                minimum_app: report.minimum_app_version.clone(),
                minimum_codex: report.minimum_bundled_codex_version.clone(),
            })
        }
        DesktopCompatibilityStatus::Unavailable => Err(ChatGptDesktopError::UnsupportedPlatform),
    }
}

fn ensure_managed_profile(profile: &ManagedProfile) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.root)?;
    if profile.root.exists() {
        if profile.marker.exists() {
            create_private_directory(&profile.root)?;
            return validate_managed_profile(profile);
        }
        let empty = fs::read_dir(&profile.root)
            .map_err(ChatGptDesktopError::InspectProfile)?
            .next()
            .is_none();
        if !empty {
            return Err(ChatGptDesktopError::UnmanagedProfile);
        }
    } else {
        create_private_directory(&profile.root)?;
    }
    let marker = ProfileMarker {
        schema_version: PROFILE_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
    };
    let serialized =
        serde_json::to_vec_pretty(&marker).map_err(ChatGptDesktopError::SerializeState)?;
    let mut file = create_private_new(&profile.marker)?;
    file.write_all(&serialized)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(ChatGptDesktopError::WriteState)?;
    Ok(())
}

fn validate_managed_profile(profile: &ManagedProfile) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.root)?;
    reject_symlink(&profile.marker)?;
    let contents = fs::read(&profile.marker).map_err(ChatGptDesktopError::ReadState)?;
    let marker: ProfileMarker =
        serde_json::from_slice(&contents).map_err(ChatGptDesktopError::ParseMarker)?;
    if marker.schema_version != PROFILE_SCHEMA_VERSION || marker.surface != SURFACE_ID {
        return Err(ChatGptDesktopError::InvalidMarker);
    }
    Ok(())
}

fn apply_session(
    profile: &ManagedProfile,
    bridge: &RunningCodexDesktopBridge,
    web_search_enabled: bool,
) -> Result<(), ChatGptDesktopError> {
    validate_managed_profile(profile)?;
    reject_orphaned_session_files(profile)?;
    let receipt = SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
        config_file: CONFIG_FILE_NAME.to_owned(),
        model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
    };
    let serialized =
        serde_json::to_vec_pretty(&receipt).map_err(ChatGptDesktopError::SerializeState)?;
    write_private_atomic(&profile.receipt, &[serialized.as_slice(), b"\n"].concat())?;
    write_private_atomic(&profile.catalog, bridge.model_catalog_json().as_bytes())?;
    let config = desktop_config(
        bridge.selected_model(),
        bridge.base_url(),
        &profile.catalog,
        web_search_enabled,
    )?;
    write_private_atomic(&profile.config, config.as_bytes()).map_err(ChatGptDesktopError::from)
}

fn desktop_config(
    selected_model: &str,
    bridge_base_url: &str,
    catalog_path: &Path,
    web_search_enabled: bool,
) -> Result<String, ChatGptDesktopError> {
    let model =
        serde_json::to_string(selected_model).map_err(ChatGptDesktopError::SerializeState)?;
    let base_url = serde_json::to_string(&format!("{}/v1", bridge_base_url.trim_end_matches('/')))
        .map_err(ChatGptDesktopError::SerializeState)?;
    let catalog = serde_json::to_string(&catalog_path.to_string_lossy())
        .map_err(ChatGptDesktopError::SerializeState)?;
    Ok(format!(
        concat!(
            "model = {}\n",
            "model_provider = \"nan_harness\"\n",
            "model_catalog_json = {}\n",
            "suppress_unstable_features_warning = true\n\n",
            "[features]\n",
            "apps = false\n",
            "standalone_web_search = {}\n",
            "responses_websockets = false\n",
            "responses_websockets_v2 = false\n\n",
            "[model_providers.nan_harness]\n",
            "name = \"nan-harness\"\n",
            "base_url = {}\n",
            "env_key = \"{}\"\n",
            "wire_api = \"responses\"\n",
            "request_max_retries = 0\n",
            "stream_max_retries = 0\n",
            "supports_websockets = false\n",
            "supports_standalone_web_search = {}\n",
            "requires_openai_auth = false\n"
        ),
        model, catalog, web_search_enabled, base_url, SESSION_TOKEN_ENVIRONMENT, web_search_enabled
    ))
}

fn restore_session(profile: &ManagedProfile) -> Result<bool, ChatGptDesktopError> {
    reject_symlink(&profile.receipt)?;
    let contents = match fs::read(&profile.receipt) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    let receipt: SessionReceipt =
        serde_json::from_slice(&contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.surface != SURFACE_ID
        || receipt.config_file != CONFIG_FILE_NAME
        || receipt.model_catalog_file != MODEL_CATALOG_FILE_NAME
    {
        return Err(ChatGptDesktopError::InvalidReceipt);
    }
    remove_file_if_present(&profile.config)?;
    remove_file_if_present(&profile.catalog)?;
    remove_file_if_present(&profile.receipt)?;
    Ok(true)
}

fn reject_orphaned_session_files(profile: &ManagedProfile) -> Result<(), ChatGptDesktopError> {
    if !profile.receipt.exists() && (profile.config.exists() || profile.catalog.exists()) {
        return Err(ChatGptDesktopError::OrphanedSessionFiles);
    }
    Ok(())
}

fn selected_model_from_config(profile: &ManagedProfile, available: &[String]) -> Option<String> {
    let contents = fs::read_to_string(&profile.config).ok()?;
    let document = toml_edit::DocumentMut::from_str(&contents).ok()?;
    let selected = document.get("model")?.as_str()?;
    available
        .iter()
        .find(|model| model.as_str() == selected)
        .cloned()
}

async fn supervise_desktop(
    installation: &ChatGptInstallation,
    profile: &ManagedProfile,
    bridge: &mut RunningCodexDesktopBridge,
    debug: bool,
    cancellation: &nan_harness_runtime::CancellationToken,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, ChatGptDesktopError> {
    let mut command = Command::new(&installation.executable);
    let mut activities = bridge.subscribe_activities();
    command
        .env("CODEX_HOME", &profile.root)
        .env_remove("CODEX_API_KEY")
        .env_remove("NAN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_CI")
        .env_remove("CODEX_THREAD_ID")
        .kill_on_drop(true);
    bridge.with_session_token(|token| {
        command.env(SESSION_TOKEN_ENVIRONMENT, token);
    });
    if debug {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut child = command.spawn().map_err(ChatGptDesktopError::StartApp)?;
    detect_singleton_race(&mut child).await?;
    let mut diagnostic_receiver = bridge.take_diagnostics();
    let handshake_deadline = tokio::time::sleep(BRIDGE_HANDSHAKE_TIMEOUT);
    tokio::pin!(handshake_deadline);
    let mut authenticated = false;
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(ChatGptDesktopError::WaitForApp)?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return Ok(exit_code(status));
            }
            signal = cancellation.cancelled() => {
                stop_chatgpt(&mut child).await?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return Ok(signal.exit_code());
            }
            bridge_result = bridge.wait() => {
                stop_chatgpt(&mut child).await?;
                drain_diagnostics(&mut diagnostic_receiver, diagnostics);
                return match bridge_result {
                    Ok(()) => Err(ChatGptDesktopError::BridgeExited),
                    Err(error) => Err(ChatGptDesktopError::Bridge(CodexDesktopBridgeError::Bridge(error))),
                };
            }
            diagnostic = diagnostic_receiver.recv() => {
                if let Some(diagnostic) = diagnostic
                    && !diagnostics.contains(&diagnostic)
                {
                    diagnostics.push(diagnostic);
                }
            }
            activity = activities.recv(), if !authenticated => {
                if matches!(
                    activity,
                    Ok(nan_harness_runtime::BridgeActivity::AuthenticatedClient)
                ) {
                    authenticated = true;
                }
            }
            () = &mut handshake_deadline, if !authenticated => {
                stop_chatgpt(&mut child).await?;
                return Err(ChatGptDesktopError::BridgeHandshakeTimeout);
            }
        }
    }
}

async fn detect_singleton_race(child: &mut Child) -> Result<(), ChatGptDesktopError> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    if let Some(status) = child.try_wait().map_err(ChatGptDesktopError::WaitForApp)? {
        return Err(classify_early_exit(status.success(), chatgpt_is_running()?));
    }
    Ok(())
}

const fn classify_early_exit(success: bool, app_running: bool) -> ChatGptDesktopError {
    if success && app_running {
        ChatGptDesktopError::SingletonRace
    } else {
        ChatGptDesktopError::AppExitedDuringStartup
    }
}

fn drain_diagnostics(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) {
    while let Ok(diagnostic) = receiver.try_recv() {
        if !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
    }
}

async fn stop_chatgpt(child: &mut Child) -> Result<(), ChatGptDesktopError> {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!("tell application id \"{APP_BUNDLE_ID}\" to quit"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    match tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(ChatGptDesktopError::WaitForApp(error)),
        Err(_) => match child.kill().await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(ChatGptDesktopError::StopApp(error)),
        },
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn require_app_stopped() -> Result<(), ChatGptDesktopError> {
    if chatgpt_is_running()? {
        Err(ChatGptDesktopError::AppAlreadyRunning)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
    let status = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "ChatGPT"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ChatGptDesktopError::InspectProcess)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(ChatGptDesktopError::ProcessInspectionFailed),
    }
}

#[cfg(target_os = "windows")]
fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
    let output = std::process::Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq ChatGPT.exe", "/FO", "CSV", "/NH"])
        .output()
        .map_err(ChatGptDesktopError::InspectProcess)?;
    if !output.status.success() {
        return Err(ChatGptDesktopError::ProcessInspectionFailed);
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains("\"ChatGPT.exe\""))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
    Err(ChatGptDesktopError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
fn discover_installation(
    explicit: Option<&Path>,
) -> Result<ChatGptInstallation, ChatGptDesktopError> {
    let candidates = if let Some(path) = explicit {
        let application = path
            .ancestors()
            .find(|candidate| candidate.extension() == Some(OsStr::new("app")))
            .map(Path::to_path_buf)
            .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        vec![application]
    } else {
        let mut candidates = vec![PathBuf::from("/Applications/ChatGPT.app")];
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join("Applications/ChatGPT.app"));
        }
        candidates
    };
    let application = candidates
        .into_iter()
        .find(|candidate| candidate.is_dir() && candidate.extension() == Some(OsStr::new("app")))
        .ok_or(ChatGptDesktopError::AppNotFound)?;
    reject_symlink(&application)?;
    let executable = application.join("Contents/MacOS/ChatGPT");
    let bundled_codex = application.join("Contents/Resources/codex");
    let info_plist = application.join("Contents/Info.plist");
    if !executable.is_file() || !bundled_codex.is_file() || !info_plist.is_file() {
        return Err(ChatGptDesktopError::InvalidInstallation);
    }
    let bundle_output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(&info_plist)
        .output()
        .map_err(ChatGptDesktopError::VersionCommand)?;
    if !bundle_output.status.success()
        || String::from_utf8_lossy(&bundle_output.stdout).trim() != APP_BUNDLE_ID
    {
        return Err(ChatGptDesktopError::InvalidInstallation);
    }
    let app_output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(&info_plist)
        .output()
        .map_err(ChatGptDesktopError::VersionCommand)?;
    if !app_output.status.success() {
        return Err(ChatGptDesktopError::VersionCommandFailed);
    }
    let app_version = parse_version_output(&String::from_utf8_lossy(&app_output.stdout))?;
    let codex_output = std::process::Command::new(&bundled_codex)
        .arg("--version")
        .output()
        .map_err(ChatGptDesktopError::VersionCommand)?;
    if !codex_output.status.success() {
        return Err(ChatGptDesktopError::VersionCommandFailed);
    }
    let bundled_codex_version =
        parse_version_output(&String::from_utf8_lossy(&codex_output.stdout))?;
    Ok(ChatGptInstallation {
        executable,
        app_version,
        bundled_codex_version,
    })
}

#[cfg(target_os = "windows")]
fn discover_installation(
    explicit: Option<&Path>,
) -> Result<ChatGptInstallation, ChatGptDesktopError> {
    let executable = if let Some(executable) = explicit {
        executable.to_path_buf()
    } else {
        let program_files = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .ok_or(ChatGptDesktopError::AppNotFound)?;
        let packages = program_files.join("WindowsApps");
        let mut roots = fs::read_dir(packages)
            .map_err(ChatGptDesktopError::InspectProfile)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("OpenAI.ChatGPT_"))
            })
            .collect::<Vec<_>>();
        roots.sort_unstable();
        roots
            .into_iter()
            .rev()
            .find_map(|root| {
                [
                    root.join("app/ChatGPT.exe"),
                    root.join("ChatGPT.exe"),
                    root.join("ChatGPT/ChatGPT.exe"),
                ]
                .into_iter()
                .find(|path| path.is_file())
            })
            .ok_or(ChatGptDesktopError::AppNotFound)?
    };
    let package_root = executable
        .ancestors()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("OpenAI.ChatGPT_"))
        })
        .ok_or(ChatGptDesktopError::InvalidInstallation)?;
    if executable.file_name().and_then(|name| name.to_str()) != Some("ChatGPT.exe") {
        return Err(ChatGptDesktopError::InvalidInstallation);
    }
    let package_name = package_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ChatGptDesktopError::UnparseableVersion)?;
    let app_version = package_name
        .split('_')
        .nth(1)
        .and_then(|version| Version::parse(version).ok())
        .ok_or(ChatGptDesktopError::UnparseableVersion)?;
    let bundled_codex = [
        package_root.join("app/resources/codex.exe"),
        package_root.join("resources/codex.exe"),
        package_root.join("codex.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .ok_or(ChatGptDesktopError::InvalidInstallation)?;
    let codex_output = std::process::Command::new(bundled_codex)
        .arg("--version")
        .output()
        .map_err(ChatGptDesktopError::VersionCommand)?;
    if !codex_output.status.success() {
        return Err(ChatGptDesktopError::VersionCommandFailed);
    }
    let bundled_codex_version =
        parse_version_output(&String::from_utf8_lossy(&codex_output.stdout))?;
    Ok(ChatGptInstallation {
        executable,
        app_version,
        bundled_codex_version,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn discover_installation(
    _explicit: Option<&Path>,
) -> Result<ChatGptInstallation, ChatGptDesktopError> {
    Err(ChatGptDesktopError::UnsupportedPlatform)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn parse_version_output(output: &str) -> Result<Version, ChatGptDesktopError> {
    output
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '+')
            })
        })
        .find_map(|candidate| Version::parse(candidate.trim_start_matches('v')).ok())
        .ok_or(ChatGptDesktopError::UnparseableVersion)
}

#[derive(Debug, Error)]
pub(crate) enum ChatGptDesktopError {
    #[error(
        "ChatGPT Desktop Preview requires the official macOS or Windows app; no official Linux distribution is available"
    )]
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
    UnsupportedPlatform,
    #[error("ChatGPT.app was not found in a supported Applications directory")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    AppNotFound,
    #[error("the ChatGPT Desktop installation is incomplete or invalid")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    InvalidInstallation,
    #[error("could not run a ChatGPT Desktop version command: {0}")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    VersionCommand(std::io::Error),
    #[error("a ChatGPT Desktop version command failed")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    VersionCommandFailed,
    #[error("could not parse the ChatGPT Desktop or bundled Codex version")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
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
        "ChatGPT Desktop did not terminate, so its launch-scoped configuration was preserved; quit it completely, then run `nan chatgpt-desktop --restore`"
    )]
    AppDidNotTerminate,
    #[error("ChatGPT Desktop exited before its managed session became ready")]
    AppExitedDuringStartup,
    #[error("could not inspect the running ChatGPT process: {0}")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    InspectProcess(std::io::Error),
    #[error("the operating system could not determine whether ChatGPT is running")]
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
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
        "managed Desktop configuration exists without a valid recovery receipt; preserve the profile and run nan chatgpt-desktop --restore after inspecting it"
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

#[cfg(test)]
mod tests {
    use super::{
        ChatGptDesktopError, ManagedProfile, PROFILE_SCHEMA_VERSION, ProfileMarker,
        SESSION_SCHEMA_VERSION, SURFACE_ID, SessionReceipt, classify_early_exit, desktop_config,
        ensure_managed_profile, parse_version_output, restore_session,
    };

    fn profile(root: &std::path::Path) -> ManagedProfile {
        ManagedProfile {
            root: root.to_path_buf(),
            marker: root.join(super::PROFILE_MARKER_NAME),
            receipt: root.join(super::SESSION_RECEIPT_NAME),
            config: root.join(super::CONFIG_FILE_NAME),
            catalog: root.join(super::MODEL_CATALOG_FILE_NAME),
        }
    }

    #[test]
    fn parses_app_and_codex_version_output() {
        assert_eq!(
            parse_version_output("26.825.51511\n")
                .expect("app version should parse")
                .to_string(),
            "26.825.51511"
        );
        assert_eq!(
            parse_version_output("codex-cli 0.151.0-alpha.7.2\n")
                .expect("Codex version should parse")
                .to_string(),
            "0.151.0-alpha.7.2"
        );
    }

    #[test]
    fn recovery_removes_only_receipt_owned_session_files() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let profile = profile(&directory.path().join("profile"));
        ensure_managed_profile(&profile).expect("profile should be created");
        std::fs::write(&profile.config, "model = \"qwen3.6\"\n").expect("config should write");
        std::fs::write(&profile.catalog, "{}\n").expect("catalog should write");
        std::fs::write(
            &profile.receipt,
            serde_json::to_vec(&SessionReceipt {
                schema_version: SESSION_SCHEMA_VERSION,
                surface: SURFACE_ID.to_owned(),
                config_file: super::CONFIG_FILE_NAME.to_owned(),
                model_catalog_file: super::MODEL_CATALOG_FILE_NAME.to_owned(),
            })
            .expect("receipt should serialize"),
        )
        .expect("receipt should write");
        std::fs::write(profile.root.join("auth.json"), "private\n")
            .expect("persistent state should write");

        assert!(restore_session(&profile).expect("recovery should succeed"));
        assert!(!profile.config.exists());
        assert!(!profile.catalog.exists());
        assert!(!profile.receipt.exists());
        assert!(profile.root.join("auth.json").exists());
    }

    #[test]
    fn invalid_recovery_receipts_preserve_every_session_file() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let profile = profile(&directory.path().join("profile"));
        ensure_managed_profile(&profile).expect("profile should be created");
        std::fs::write(&profile.config, "model = \"qwen3.6\"\n").expect("config should write");
        std::fs::write(&profile.catalog, "{}\n").expect("catalog should write");
        std::fs::write(
            &profile.receipt,
            serde_json::to_vec(&SessionReceipt {
                schema_version: SESSION_SCHEMA_VERSION,
                surface: SURFACE_ID.to_owned(),
                config_file: "../config.toml".to_owned(),
                model_catalog_file: super::MODEL_CATALOG_FILE_NAME.to_owned(),
            })
            .expect("receipt should serialize"),
        )
        .expect("receipt should write");

        assert!(matches!(
            restore_session(&profile),
            Err(ChatGptDesktopError::InvalidReceipt)
        ));
        assert!(profile.config.exists());
        assert!(profile.catalog.exists());
        assert!(profile.receipt.exists());
    }

    #[test]
    fn desktop_config_contains_only_loopback_routing_and_a_token_reference() {
        let config = desktop_config(
            "qwen3.6",
            "http://127.0.0.1:43123",
            std::path::Path::new("/private/profile/nan-model-catalog.json"),
            true,
        )
        .expect("desktop config should render");
        let document = config
            .parse::<toml_edit::DocumentMut>()
            .expect("desktop config should be valid TOML");

        assert_eq!(document["model"].as_str(), Some("qwen3.6"));
        assert_eq!(
            document["model_providers"]["nan_harness"]["base_url"].as_str(),
            Some("http://127.0.0.1:43123/v1")
        );
        assert_eq!(
            document["model_providers"]["nan_harness"]["env_key"].as_str(),
            Some(super::SESSION_TOKEN_ENVIRONMENT)
        );
        assert_eq!(document["features"]["apps"].as_bool(), Some(false));
        assert!(!config.contains("NAN_API_KEY"));
        assert!(!config.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn invalid_profile_marker_is_rejected_without_claiming_the_directory() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let profile = profile(&directory.path().join("profile"));
        std::fs::create_dir_all(&profile.root).expect("profile should exist");
        std::fs::write(profile.root.join("user-owned"), "keep\n").expect("user file should write");

        assert!(matches!(
            ensure_managed_profile(&profile),
            Err(ChatGptDesktopError::UnmanagedProfile)
        ));
        assert!(profile.root.join("user-owned").exists());
    }

    #[test]
    fn marker_contract_is_strict() {
        let marker = ProfileMarker {
            schema_version: PROFILE_SCHEMA_VERSION,
            surface: SURFACE_ID.to_owned(),
        };
        let json = serde_json::to_value(marker).expect("marker should serialize");
        assert_eq!(json["schemaVersion"], PROFILE_SCHEMA_VERSION);
        assert_eq!(json["surface"], SURFACE_ID);
    }

    #[test]
    fn early_exit_distinguishes_a_singleton_race_from_a_failed_start() {
        assert!(matches!(
            classify_early_exit(true, true),
            ChatGptDesktopError::SingletonRace
        ));
        assert!(matches!(
            classify_early_exit(true, false),
            ChatGptDesktopError::AppExitedDuringStartup
        ));
        assert!(matches!(
            classify_early_exit(false, true),
            ChatGptDesktopError::AppExitedDuringStartup
        ));
    }
}
