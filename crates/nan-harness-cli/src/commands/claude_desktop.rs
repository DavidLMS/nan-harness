use crate::app::ClaudeDesktopArgs;
use crate::commands::credentials;
use crate::commands::persistence::PersistenceManager;
use crate::error::CliError;
use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy};
use nan_harness_private_fs::open_private_new;
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, ClaudeAutoModeReviewStage, DesktopCompatibilityEvidence,
    DesktopCompatibilityStatus, RunningClaudeDesktopBridge, classify_desktop_version,
    desktop_compatibility, start_claude_desktop_bridge,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::future::Future;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;

const PROFILE_ID: &str = "6e616e68-6172-4e65-8000-000000000001";
const PROFILE_NAME: &str = "NaN Harness";
const RECEIPT_SCHEMA: u8 = 2;
const DOCUMENT_IDS: [&str; 4] = [
    "normal-config",
    "third-party-config",
    "profile-meta",
    "profile",
];

pub(crate) async fn run(
    arguments: &ClaudeDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    if arguments.dry_run {
        return print_dry_run(arguments);
    }
    let compatibility =
        desktop_compatibility(DesktopHarnessKind::Claude).map_err(ClaudeDesktopError::from)?;
    match classify_desktop_version(&compatibility, None) {
        DesktopCompatibilityStatus::ContractOnly => eprintln!(
            "warning: Claude Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
        ),
        DesktopCompatibilityStatus::Unavailable => {
            return Err(ClaudeDesktopError::UnsupportedPlatform.into());
        }
        DesktopCompatibilityStatus::Tested
        | DesktopCompatibilityStatus::NewerUntested
        | DesktopCompatibilityStatus::OlderUnsupported => {}
    }
    debug_assert_ne!(
        compatibility.evidence,
        DesktopCompatibilityEvidence::Unavailable
    );
    let manager = PersistenceManager::from_environment()?;
    let remembered_model = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Claude)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let requested_model = arguments.model.as_deref().or(remembered_model.as_deref());
    let platform = DesktopPlatform::current()?;
    let paths = DesktopPaths::from_environment(platform)?;
    let process = SystemDesktopProcess::new(platform, arguments.executable.clone());
    if arguments.restore {
        return restore_command(&paths, &process);
    }
    let _lock = prepare_session_lock(&paths, &process)?;
    ensure_no_pending_recovery(&paths)?;
    if process.is_running()? {
        return Err(ClaudeDesktopError::AlreadyRunning.into());
    }
    let mut config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let discovered_models = config.model_catalog.take();
    let bridge = start_claude_desktop_bridge(
        &config.config,
        discovered_models,
        requested_model,
        arguments.show_auto,
        !arguments.search.no_search,
    )
    .await
    .map_err(ClaudeDesktopError::from)?;
    let selected_model = bridge.selected_model().to_owned();
    let result = run_ready_session(&paths, &process, &bridge, arguments.show_auto).await;
    let shutdown = bridge.shutdown_with_usage().await;
    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            append_diagnostics(bridge_diagnostics, diagnostics);
            if let Err(error) =
                manager.save_last_desktop_selection(DesktopHarnessKind::Claude, &selected_model)
            {
                eprintln!("warning: could not save the last Desktop model: {error}");
            }
            let outcome = if code == 0 {
                nan_harness_runtime::ExecutionOutcome::Succeeded
            } else {
                nan_harness_runtime::ExecutionOutcome::Failed
            };
            if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
                eprintln!("{summary}");
            }
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(ClaudeDesktopError::Bridge(error).into()),
    }
}

fn print_dry_run(arguments: &ClaudeDesktopArgs) -> Result<i32, CliError> {
    let plan = dry_run_plan(arguments);
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(ClaudeDesktopError::SerializeReceipt)?
    );
    Ok(0)
}

fn dry_run_plan(arguments: &ClaudeDesktopArgs) -> DesktopLaunchPlan {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Claude,
        DesktopTransport::AnthropicBridge,
    );
    plan.executable.clone_from(&arguments.executable);
    plan.selected_model.clone_from(&arguments.model);
    plan.private_diagnostics = arguments.show_auto;
    plan.web_search_policy = if arguments.search.no_search {
        WebSearchPolicy::Disabled
    } else if arguments.search.force_search {
        WebSearchPolicy::Force
    } else {
        WebSearchPolicy::Auto
    };
    plan
}

fn restore_command(paths: &DesktopPaths, process: &SystemDesktopProcess) -> Result<i32, CliError> {
    let _lock = SessionLock::acquire(&paths.lock)?;
    if process.is_running()? {
        return Err(ClaudeDesktopError::AlreadyRunning.into());
    }
    match restore_receipt(paths) {
        Ok(()) => eprintln!("Claude Desktop configuration restored."),
        Err(ClaudeDesktopError::NoReceipt) => {
            eprintln!("No Claude Desktop session needs recovery.");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(0)
}

fn append_diagnostics(target: &mut Vec<BridgeDiagnostic>, diagnostics: Vec<BridgeDiagnostic>) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

async fn run_ready_session(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
    bridge: &RunningClaudeDesktopBridge,
    show_auto: bool,
) -> Result<i32, ClaudeDesktopError> {
    let receipt = Receipt::capture(paths)?;
    if let Err(error) = receipt.write(&paths.receipt) {
        Receipt::remove_backups(paths);
        return Err(error);
    }
    let apply = bridge.with_session_token(|token| apply_gateway(paths, bridge.base_url(), token));
    if let Err(error) = apply {
        return restore_after(paths, Err(error));
    }
    let activities = show_auto.then(|| bridge.subscribe_activities());
    if let Err(error) = process.launch() {
        return complete_and_restore(paths, process, Err(error)).await;
    }
    eprintln!("{}", launch_message(show_auto));
    let activity_logger =
        activities.map(|activities| tokio::spawn(log_bridge_activities(activities)));
    let completion = wait_for_exit_or_signal(process).await;
    if let Some(activity_logger) = activity_logger {
        activity_logger.abort();
    }
    complete_and_restore(paths, process, completion).await
}

fn launch_message(show_auto: bool) -> &'static str {
    if show_auto {
        "Claude Desktop launched through NaN. Auto traces will appear here and may contain private data."
    } else {
        "Claude Desktop launched through NaN."
    }
}

async fn log_bridge_activities(mut activities: tokio::sync::broadcast::Receiver<BridgeActivity>) {
    loop {
        match activities.recv().await {
            Ok(activity) => eprintln!("{}", render_bridge_activity(&activity)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                eprintln!("[Auto] {skipped} permission review events were omitted.");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn render_bridge_activity(activity: &BridgeActivity) -> String {
    match activity {
        BridgeActivity::AuthenticatedClient => {
            "[Bridge] Claude Desktop authenticated to the isolated NaN bridge.".to_owned()
        }
        BridgeActivity::ClaudeAutoModeReview {
            review_id,
            stage,
            model_id,
            request,
        } => {
            let stage = match stage {
                ClaudeAutoModeReviewStage::Initial => "stage 1",
                ClaudeAutoModeReviewStage::FollowUp => "stage 2",
            };
            format!(
                "[Auto #{review_id}] Claude requested a permission review ({stage}, classifier {model_id}).\n[Auto #{review_id}] NaN request:\n{}",
                render_trace_payload(request)
            )
        }
        BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id,
            status,
            response,
        } => {
            format!(
                "[Auto #{review_id}] NaN response (HTTP {status}):\n{}",
                render_trace_payload(response)
            )
        }
        BridgeActivity::ClaudeAutoModeReviewFailed {
            review_id,
            error_code,
        } => {
            format!(
                "[Auto #{review_id}] NaN request failed before a response was received ({error_code})."
            )
        }
    }
}

fn render_trace_payload(payload: &nan_harness_runtime::ClaudeAutoModeTracePayload) -> String {
    payload.with_contents(|contents| {
        serde_json::from_str::<Value>(contents).map_or_else(
            |_| contents.to_owned(),
            |value| serde_json::to_string_pretty(&value).unwrap_or_else(|_| contents.to_owned()),
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    Exited,
    Signaled(i32),
}

async fn wait_for_exit_or_signal(
    process: &impl DesktopProcess,
) -> Result<WaitOutcome, ClaudeDesktopError> {
    let mut observed_running = false;
    let mut startup_polls = 0_u8;
    let signal = termination_signal();
    tokio::pin!(signal);
    loop {
        if let Some(outcome) =
            observe_process_state(process, &mut observed_running, &mut startup_polls)?
        {
            return Ok(outcome);
        }
        if let Some(signal) = wait_for_poll_or_signal(signal.as_mut()).await {
            return Ok(WaitOutcome::Signaled(signal));
        }
    }
}

fn observe_process_state(
    process: &impl DesktopProcess,
    observed_running: &mut bool,
    startup_polls: &mut u8,
) -> Result<Option<WaitOutcome>, ClaudeDesktopError> {
    match process.is_running()? {
        true => *observed_running = true,
        false if *observed_running => return Ok(Some(WaitOutcome::Exited)),
        false => check_startup_limit(startup_polls)?,
    }
    Ok(None)
}

fn check_startup_limit(startup_polls: &mut u8) -> Result<(), ClaudeDesktopError> {
    *startup_polls = startup_polls.saturating_add(1);
    if *startup_polls >= 40 {
        return Err(ClaudeDesktopError::DidNotStart);
    }
    Ok(())
}

async fn wait_for_poll_or_signal<F>(signal: std::pin::Pin<&mut F>) -> Option<i32>
where
    F: Future<Output = i32>,
{
    tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(125)) => None,
        signal = signal => Some(signal),
    }
}

async fn terminate_and_wait(process: &impl DesktopProcess) -> Result<(), ClaudeDesktopError> {
    let graceful = process.terminate();
    if graceful.is_ok() && wait_for_termination(process).await.is_ok() {
        return Ok(());
    }
    process.force_terminate()?;
    wait_for_termination(process).await
}

async fn complete_and_restore(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
    completion: Result<WaitOutcome, ClaudeDesktopError>,
) -> Result<i32, ClaudeDesktopError> {
    match completion {
        Ok(WaitOutcome::Exited) => restore_after(paths, Ok(0)),
        Ok(WaitOutcome::Signaled(exit_code)) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Ok(exit_code))
        }
        Err(error) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Err(error))
        }
    }
}

async fn wait_for_termination(process: &impl DesktopProcess) -> Result<(), ClaudeDesktopError> {
    for _ in 0..120 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    Err(ClaudeDesktopError::DidNotTerminate)
}

fn restore_after(
    paths: &DesktopPaths,
    completion: Result<i32, ClaudeDesktopError>,
) -> Result<i32, ClaudeDesktopError> {
    match (completion, restore_receipt(paths)) {
        (Ok(exit_code), Ok(())) => Ok(exit_code),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

#[cfg(unix)]
async fn termination_signal() -> i32 {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return 130;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => 130,
        _ = terminate.recv() => 143,
    }
}

#[cfg(not(unix))]
async fn termination_signal() -> i32 {
    let _ = tokio::signal::ctrl_c().await;
    130
}

trait DesktopProcess {
    fn is_running(&self) -> Result<bool, ClaudeDesktopError>;
    fn ensure_available(&self) -> Result<(), ClaudeDesktopError>;
    fn launch(&self) -> Result<(), ClaudeDesktopError>;
    fn terminate(&self) -> Result<(), ClaudeDesktopError>;
    fn force_terminate(&self) -> Result<(), ClaudeDesktopError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopPlatform {
    Macos,
    Linux,
    Windows,
}

impl DesktopPlatform {
    fn current() -> Result<Self, ClaudeDesktopError> {
        if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "windows") {
            Ok(Self::Windows)
        } else {
            Err(ClaudeDesktopError::UnsupportedPlatform)
        }
    }

    const fn installation_hint(self) -> &'static str {
        match self {
            Self::Macos => "macOS (/Applications or ~/Applications)",
            Self::Linux => "Linux (`claude-desktop` on PATH)",
            Self::Windows => "Windows (registered `claude://` handler or per-user installation)",
        }
    }
}

struct SystemDesktopProcess {
    platform: DesktopPlatform,
    executable: Option<PathBuf>,
}

impl SystemDesktopProcess {
    const fn new(platform: DesktopPlatform, executable: Option<PathBuf>) -> Self {
        Self {
            platform,
            executable,
        }
    }
}

impl DesktopProcess for SystemDesktopProcess {
    fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => process_matches(
                "/usr/bin/pgrep",
                &["-f", "Claude.app/Contents/MacOS/Claude"],
            ),
            DesktopPlatform::Linux => linux_desktop_running(),
            DesktopPlatform::Windows => windows_desktop_running(),
        }
    }

    fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
        if let Some(executable) = &self.executable {
            if is_executable_file(executable)
                || (self.platform == DesktopPlatform::Macos
                    && executable.is_dir()
                    && executable.extension().is_some_and(|value| value == "app"))
            {
                return Ok(());
            }
            return Err(ClaudeDesktopError::AppNotFound {
                platform: self.platform.installation_hint(),
            });
        }
        let available = match self.platform {
            DesktopPlatform::Macos => find_macos_app().is_some(),
            DesktopPlatform::Linux => find_executable("claude-desktop").is_some(),
            DesktopPlatform::Windows => {
                find_windows_app().is_some() || windows_protocol_registered()?
            }
        };
        if available {
            Ok(())
        } else {
            Err(ClaudeDesktopError::AppNotFound {
                platform: self.platform.installation_hint(),
            })
        }
    }

    fn launch(&self) -> Result<(), ClaudeDesktopError> {
        if let Some(executable) = &self.executable {
            if self.platform == DesktopPlatform::Macos && executable.is_dir() {
                return run_launcher("/usr/bin/open", &[executable.as_os_str()]);
            }
            return Command::new(executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(ClaudeDesktopError::Launch);
        }
        match self.platform {
            DesktopPlatform::Macos => {
                let app = find_macos_app().ok_or(ClaudeDesktopError::AppNotFound {
                    platform: self.platform.installation_hint(),
                })?;
                run_launcher("/usr/bin/open", &[app.as_os_str()])
            }
            DesktopPlatform::Linux => {
                let executable =
                    find_executable("claude-desktop").ok_or(ClaudeDesktopError::AppNotFound {
                        platform: self.platform.installation_hint(),
                    })?;
                Command::new(executable)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map(|_| ())
                    .map_err(ClaudeDesktopError::Launch)
            }
            DesktopPlatform::Windows => {
                if let Some(executable) = find_windows_app() {
                    Command::new(executable)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map(|_| ())
                        .map_err(ClaudeDesktopError::Launch)
                } else {
                    run_launcher(
                        "explorer.exe",
                        &[std::ffi::OsStr::new("claude://claude.ai/new")],
                    )
                }
            }
        }
    }

    fn terminate(&self) -> Result<(), ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => terminate_macos(),
            DesktopPlatform::Linux => terminate_linux(LinuxSignal::Terminate),
            DesktopPlatform::Windows => {
                terminate_matches("taskkill.exe", &["/IM", "Claude.exe", "/T"])
            }
        }
    }

    fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => terminate_matches(
                "/usr/bin/pkill",
                &["-KILL", "-f", "Claude.app/Contents/MacOS/Claude"],
            ),
            DesktopPlatform::Linux => terminate_linux(LinuxSignal::Kill),
            DesktopPlatform::Windows => {
                terminate_matches("taskkill.exe", &["/F", "/IM", "Claude.exe", "/T"])
            }
        }
    }
}

fn process_matches(command: &str, arguments: &[&str]) -> Result<bool, ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::ProcessCheck)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(ClaudeDesktopError::ProcessCheckFailed(status.code())),
    }
}

#[derive(Debug, Clone, Copy)]
enum LinuxSignal {
    Terminate,
    Kill,
}

#[cfg(target_os = "linux")]
fn linux_desktop_running() -> Result<bool, ClaudeDesktopError> {
    linux_desktop_pids().map(|process_ids| !process_ids.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn linux_desktop_running() -> Result<bool, ClaudeDesktopError> {
    Err(ClaudeDesktopError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn linux_desktop_pids() -> Result<Vec<nix::unistd::Pid>, ClaudeDesktopError> {
    let entries = fs::read_dir("/proc").map_err(ClaudeDesktopError::ProcessCheck)?;
    Ok(entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let process_id = entry.file_name().to_string_lossy().parse::<i32>().ok()?;
            let command = fs::read_to_string(entry.path().join("comm")).ok()?;
            (command.trim() == "claude-desktop").then(|| nix::unistd::Pid::from_raw(process_id))
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn terminate_linux(signal: LinuxSignal) -> Result<(), ClaudeDesktopError> {
    let signal = match signal {
        LinuxSignal::Terminate => nix::sys::signal::Signal::SIGTERM,
        LinuxSignal::Kill => nix::sys::signal::Signal::SIGKILL,
    };
    for process_id in linux_desktop_pids()? {
        if let Err(error) = nix::sys::signal::kill(process_id, signal) {
            if error == nix::errno::Errno::ESRCH {
                continue;
            }
            return Err(ClaudeDesktopError::Terminate(
                std::io::Error::from_raw_os_error(error as i32),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn terminate_linux(_signal: LinuxSignal) -> Result<(), ClaudeDesktopError> {
    Err(ClaudeDesktopError::UnsupportedPlatform)
}

fn windows_desktop_running() -> Result<bool, ClaudeDesktopError> {
    let output = Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq Claude.exe", "/FO", "CSV", "/NH"])
        .output()
        .map_err(ClaudeDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(ClaudeDesktopError::ProcessCheckFailed(output.status.code()));
    }
    Ok(tasklist_reports_desktop(&output.stdout))
}

fn tasklist_reports_desktop(output: &[u8]) -> bool {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.trim_start().split(',').next())
        .map(|image_name| image_name.trim_matches('"'))
        .any(|image_name| image_name.eq_ignore_ascii_case("Claude.exe"))
}

fn run_launcher(command: &str, arguments: &[&std::ffi::OsStr]) -> Result<(), ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .status()
        .map_err(ClaudeDesktopError::Launch)?;
    if status.success() {
        Ok(())
    } else {
        Err(ClaudeDesktopError::LaunchFailed(status.code()))
    }
}

fn terminate_matches(command: &str, arguments: &[&str]) -> Result<(), ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::Terminate)?;
    if matches!(status.code(), Some(0 | 1 | 128)) {
        Ok(())
    } else {
        Err(ClaudeDesktopError::TerminateFailed(status.code()))
    }
}

fn terminate_macos() -> Result<(), ClaudeDesktopError> {
    let graceful = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application id \"com.anthropic.claudefordesktop\" to quit",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::Terminate)?;
    if graceful.success() {
        return Ok(());
    }
    terminate_matches(
        "/usr/bin/pkill",
        &["-TERM", "-f", "Claude.app/Contents/MacOS/Claude"],
    )
}

fn find_macos_app() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/Claude.app")];
    if let Some(home) = user_home_directory() {
        candidates.push(home.join("Applications/Claude.app"));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| is_executable_file(path))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_windows_app() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidates = [
        local.join("Programs/Claude/Claude.exe"),
        local.join("Programs/Claude Desktop/Claude.exe"),
        local.join("Claude/Claude.exe"),
        local.join("Claude Desktop/Claude.exe"),
        local.join("AnthropicClaude/Claude.exe"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| {
            find_versioned_windows_app(&local.join("AnthropicClaude"))
                .or_else(|| find_versioned_windows_app(&local.join("Programs/Claude")))
                .or_else(|| find_versioned_windows_app(&local.join("Programs/Claude Desktop")))
        })
}

fn find_versioned_windows_app(root: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("app-"))
        .map(|entry| entry.path().join("Claude.exe"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.pop()
}

fn windows_protocol_registered() -> Result<bool, ClaudeDesktopError> {
    for key in [
        r"HKCU\Software\Classes\claude\shell\open\command",
        r"HKCR\claude\shell\open\command",
    ] {
        let status = Command::new("reg.exe")
            .args(["query", key, "/ve"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(ClaudeDesktopError::ProcessCheck)?;
        match status.code() {
            Some(0) => return Ok(true),
            Some(1) => {}
            _ => return Err(ClaudeDesktopError::ProcessCheckFailed(status.code())),
        }
    }
    Ok(false)
}

fn prepare_session_lock(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
) -> Result<SessionLock, ClaudeDesktopError> {
    process.ensure_available()?;
    SessionLock::acquire(&paths.lock)
}

#[derive(Debug)]
struct DesktopPaths {
    normal_config: PathBuf,
    third_party_config: PathBuf,
    meta: PathBuf,
    profile: PathBuf,
    receipt: PathBuf,
    backup_directory: PathBuf,
    lock: PathBuf,
}

impl DesktopPaths {
    fn from_environment(platform: DesktopPlatform) -> Result<Self, ClaudeDesktopError> {
        let environment = DesktopEnvironment::current();
        Self::from_platform_environment(platform, &environment)
    }

    fn from_platform_environment(
        platform: DesktopPlatform,
        environment: &DesktopEnvironment,
    ) -> Result<Self, ClaudeDesktopError> {
        let state_override = environment.nan_config.as_deref();
        match platform {
            DesktopPlatform::Macos => {
                let support = environment
                    .home
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingHome)?
                    .join("Library/Application Support");
                let state =
                    state_override.map_or_else(|| support.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &support.join("Claude"),
                    &support.join("Claude-3p"),
                    &state,
                ))
            }
            DesktopPlatform::Linux => {
                let config = environment
                    .xdg_config
                    .clone()
                    .or_else(|| environment.home.as_deref().map(|home| home.join(".config")));
                let config = config.ok_or(ClaudeDesktopError::MissingHome)?;
                let state =
                    state_override.map_or_else(|| config.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &config.join("Claude"),
                    &config.join("Claude-3p"),
                    &state,
                ))
            }
            DesktopPlatform::Windows => {
                let roaming = environment
                    .app_data
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingPlatformDirectory("APPDATA"))?;
                let local = environment
                    .local_app_data
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingPlatformDirectory("LOCALAPPDATA"))?;
                let state =
                    state_override.map_or_else(|| roaming.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &roaming.join("Claude"),
                    &local.join("Claude-3p"),
                    &state,
                ))
            }
        }
    }

    fn new(normal_root: &Path, third_party_root: &Path, state: &Path) -> Self {
        Self {
            normal_config: normal_root.join("claude_desktop_config.json"),
            third_party_config: third_party_root.join("claude_desktop_config.json"),
            meta: third_party_root.join("configLibrary/_meta.json"),
            profile: third_party_root.join(format!("configLibrary/{PROFILE_ID}.json")),
            receipt: state.join("claude-desktop-receipt.json"),
            backup_directory: state.join("claude-desktop-backup"),
            lock: state.join("claude-desktop.lock"),
        }
    }

    fn documents(&self) -> [&Path; 4] {
        [
            &self.normal_config,
            &self.third_party_config,
            &self.meta,
            &self.profile,
        ]
    }
}

#[derive(Debug, Default)]
struct DesktopEnvironment {
    home: Option<PathBuf>,
    app_data: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    nan_config: Option<PathBuf>,
}

impl DesktopEnvironment {
    fn current() -> Self {
        Self {
            home: user_home_directory(),
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            xdg_config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            nan_config: std::env::var_os("NAN_HARNESS_CONFIG_DIR").map(PathBuf::from),
        }
    }
}

fn user_home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn ensure_no_pending_recovery(paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
    reject_symlink(&paths.receipt)?;
    reject_symlink(&paths.backup_directory)?;
    if paths.receipt.exists() {
        return Err(ClaudeDesktopError::OrphanReceipt);
    }
    if paths.backup_directory.exists() {
        return Err(ClaudeDesktopError::OrphanBackup);
    }
    Ok(())
}

struct SessionLock {
    file: File,
}

impl SessionLock {
    fn acquire(path: &Path) -> Result<Self, ClaudeDesktopError> {
        let parent = path.parent().ok_or(ClaudeDesktopError::InvalidStatePath)?;
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(ClaudeDesktopError::CreateDirectory)?;
        reject_symlink(path)?;
        let mut file = match open_private_new(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(ClaudeDesktopError::Lock)?,
            Err(error) => return Err(ClaudeDesktopError::Lock(error)),
        };
        nan_harness_private_fs::restrict_file(&mut file)
            .map_err(ClaudeDesktopError::Permissions)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(ClaudeDesktopError::ConcurrentSession);
            }
            Err(TryLockError::Error(error)) => {
                return Err(ClaudeDesktopError::Lock(error));
            }
        }
        Ok(Self { file })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Receipt {
    schema: u8,
    snapshots: Vec<Snapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    document_id: String,
    existed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[cfg(unix)]
    mode: Option<u32>,
}

impl Receipt {
    fn capture(paths: &DesktopPaths) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(&paths.backup_directory)?;
        if paths.backup_directory.exists() {
            return Err(ClaudeDesktopError::OrphanBackup);
        }
        let state_directory = paths
            .backup_directory
            .parent()
            .ok_or(ClaudeDesktopError::InvalidStatePath)?;
        nan_harness_private_fs::create_private_dir_all(state_directory)
            .map_err(ClaudeDesktopError::CreateBackupDirectory)?;
        nan_harness_private_fs::create_private_dir(&paths.backup_directory)
            .map_err(ClaudeDesktopError::CreateBackupDirectory)?;
        let result = paths
            .documents()
            .into_iter()
            .zip(DOCUMENT_IDS)
            .enumerate()
            .map(|(index, (path, document_id))| {
                Snapshot::capture(path, document_id, index, &paths.backup_directory)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|snapshots| Self {
                schema: RECEIPT_SCHEMA,
                snapshots,
            });
        if result.is_err() {
            let _ = fs::remove_dir_all(&paths.backup_directory);
        }
        result
    }

    fn write(&self, path: &Path) -> Result<(), ClaudeDesktopError> {
        reject_symlink(path)?;
        let payload = serde_json::to_vec(self).map_err(ClaudeDesktopError::SerializeReceipt)?;
        atomic_write(path, &payload, None, true)
    }

    fn read(path: &Path) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(path)?;
        let payload = fs::read(path).map_err(ClaudeDesktopError::ReadReceipt)?;
        let receipt: Self =
            serde_json::from_slice(&payload).map_err(ClaudeDesktopError::ParseReceipt)?;
        if receipt.schema != RECEIPT_SCHEMA
            || receipt.snapshots.len() != DOCUMENT_IDS.len()
            || receipt
                .snapshots
                .iter()
                .zip(DOCUMENT_IDS)
                .any(|(snapshot, expected)| snapshot.document_id != expected)
        {
            return Err(ClaudeDesktopError::UnsupportedReceipt);
        }
        Ok(receipt)
    }

    fn restore(&self, paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
        for (snapshot, path) in self.snapshots.iter().zip(paths.documents()) {
            snapshot.restore(path, &paths.backup_directory)?;
        }
        Ok(())
    }

    fn remove_backups(paths: &DesktopPaths) {
        let _ = fs::remove_dir_all(&paths.backup_directory);
    }
}

impl Snapshot {
    fn capture(
        path: &Path,
        document_id: &str,
        index: usize,
        backup_directory: &Path,
    ) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(path)?;
        match fs::read(path) {
            Ok(contents) => {
                let metadata = fs::metadata(path).map_err(ClaudeDesktopError::ReadConfig)?;
                let backup_file = format!("document-{index}.backup");
                write_private_new(&backup_directory.join(&backup_file), &contents)?;
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt as _;
                    Some(metadata.permissions().mode())
                };
                Ok(Self {
                    document_id: document_id.to_owned(),
                    existed: true,
                    backup_file: Some(backup_file),
                    sha256: Some(sha256(&contents)),
                    #[cfg(unix)]
                    mode,
                })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self {
                document_id: document_id.to_owned(),
                existed: false,
                backup_file: None,
                sha256: None,
                #[cfg(unix)]
                mode: None,
            }),
            Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
        }
    }

    fn restore(&self, path: &Path, backup_directory: &Path) -> Result<(), ClaudeDesktopError> {
        reject_symlink(path)?;
        if !self.existed {
            if self.backup_file.is_some() || self.sha256.is_some() {
                return Err(ClaudeDesktopError::UnsupportedReceipt);
            }
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(ClaudeDesktopError::Restore(error)),
            };
        }
        let backup_file = self
            .backup_file
            .as_deref()
            .ok_or(ClaudeDesktopError::UnsupportedReceipt)?;
        if Path::new(backup_file)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(backup_file)
        {
            return Err(ClaudeDesktopError::UnsupportedReceipt);
        }
        let backup_path = backup_directory.join(backup_file);
        reject_symlink(&backup_path)?;
        let contents = fs::read(backup_path).map_err(ClaudeDesktopError::ReadBackup)?;
        let actual_sha256 = sha256(&contents);
        if self.sha256.as_deref() != Some(actual_sha256.as_str()) {
            return Err(ClaudeDesktopError::BackupHashMismatch);
        }
        #[cfg(unix)]
        let permissions = self.mode.map(|mode| {
            use std::os::unix::fs::PermissionsExt as _;
            Permissions::from_mode(mode)
        });
        #[cfg(not(unix))]
        let permissions = None;
        atomic_write(path, &contents, permissions.as_ref(), false)
    }
}

fn restore_receipt(paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
    reject_symlink(&paths.receipt)?;
    reject_symlink(&paths.backup_directory)?;
    if !paths.receipt.exists() {
        return if paths.backup_directory.exists() {
            Err(ClaudeDesktopError::OrphanBackup)
        } else {
            Err(ClaudeDesktopError::NoReceipt)
        };
    }
    let receipt = Receipt::read(&paths.receipt)?;
    receipt.restore(paths)?;
    fs::remove_file(&paths.receipt).map_err(ClaudeDesktopError::RemoveReceipt)?;
    fs::remove_dir_all(&paths.backup_directory).map_err(ClaudeDesktopError::RemoveBackup)
}

fn apply_gateway(
    paths: &DesktopPaths,
    base_url: &str,
    token: &str,
) -> Result<(), ClaudeDesktopError> {
    let mut documents = paths
        .documents()
        .into_iter()
        .map(read_json_object)
        .collect::<Result<Vec<_>, _>>()?;
    documents[0].insert("deploymentMode".to_owned(), json!("3p"));
    documents[1].insert("deploymentMode".to_owned(), json!("3p"));

    documents[2].insert("appliedId".to_owned(), json!(PROFILE_ID));
    let entries = documents[2]
        .remove("entries")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut entries = entries
        .into_iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID))
        .collect::<Vec<_>>();
    entries.push(json!({"id": PROFILE_ID, "name": PROFILE_NAME}));
    documents[2].insert("entries".to_owned(), Value::Array(entries));

    let profile = &mut documents[3];
    profile.insert("inferenceProvider".to_owned(), json!("gateway"));
    profile.insert("inferenceGatewayBaseUrl".to_owned(), json!(base_url));
    profile.insert("inferenceGatewayApiKey".to_owned(), json!(token));
    profile.insert("inferenceGatewayAuthScheme".to_owned(), json!("bearer"));
    profile.insert("deploymentDisplayName".to_owned(), json!(PROFILE_NAME));
    profile.insert("modelDiscoveryEnabled".to_owned(), json!(true));
    profile.insert("chatTabEnabled".to_owned(), json!(true));
    profile.insert("autoModeEnabled".to_owned(), json!(true));
    profile.insert("disableDeploymentModeChooser".to_owned(), json!(true));
    profile.insert("coworkEgressAllowedHosts".to_owned(), json!(["*"]));
    profile.remove("inferenceModels");

    for (document, path) in documents.into_iter().zip(paths.documents()) {
        let mut payload =
            serde_json::to_vec_pretty(&document).map_err(ClaudeDesktopError::SerializeConfig)?;
        payload.push(b'\n');
        let permissions = existing_permissions(path)?;
        atomic_write(path, &payload, permissions.as_ref(), false)?;
    }
    Ok(())
}

fn sha256(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn write_private_new(path: &Path, payload: &[u8]) -> Result<(), ClaudeDesktopError> {
    let mut file = open_private_new(path).map_err(ClaudeDesktopError::WriteBackup)?;
    file.write_all(payload)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(ClaudeDesktopError::WriteBackup)
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, ClaudeDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice::<Value>(&contents)
            .map_err(ClaudeDesktopError::ParseConfig)?
            .as_object()
            .cloned()
            .ok_or(ClaudeDesktopError::ConfigRoot),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

fn existing_permissions(path: &Path) -> Result<Option<Permissions>, ClaudeDesktopError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

fn atomic_write(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
    private: bool,
) -> Result<(), ClaudeDesktopError> {
    let parent = path.parent().ok_or(ClaudeDesktopError::InvalidStatePath)?;
    if private {
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(ClaudeDesktopError::CreateDirectory)?;
    } else {
        fs::create_dir_all(parent).map_err(ClaudeDesktopError::CreateDirectory)?;
    }
    reject_symlink(path)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-")
        .make_in(parent, open_private_new)
        .map_err(ClaudeDesktopError::Write)?;
    temporary
        .write_all(payload)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(ClaudeDesktopError::Write)?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(ClaudeDesktopError::Permissions)?;
    }
    temporary
        .persist(path)
        .map_err(|error| ClaudeDesktopError::Write(error.error))?;
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), ClaudeDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ClaudeDesktopError::UnsafeSymlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

#[derive(Debug, Error)]
pub(crate) enum ClaudeDesktopError {
    #[error("Claude Desktop integration is available only on macOS, Linux, and Windows")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "Claude Desktop is already running; quit it completely, then re-run `nan claude-desktop`"
    )]
    AlreadyRunning,
    #[error("another `nan claude-desktop` session is active")]
    ConcurrentSession,
    #[error(
        "an interrupted Claude Desktop session needs recovery; run `nan claude-desktop --restore`"
    )]
    OrphanReceipt,
    #[error("no interrupted Claude Desktop configuration receipt was found")]
    NoReceipt,
    #[error("Claude Desktop did not start; its original configuration has been restored")]
    DidNotStart,
    #[error(
        "Claude Desktop did not quit, so its configuration was not restored; quit it completely, then run `nan claude-desktop --restore`"
    )]
    DidNotTerminate,
    #[error(
        "Claude Desktop was not found for {platform}; install the official app from https://support.claude.com/es/articles/10065433-instalar-claude-desktop"
    )]
    AppNotFound { platform: &'static str },
    #[error(transparent)]
    Bridge(#[from] nan_harness_runtime::ClaudeDesktopBridgeError),
    #[error("could not determine the current user's home directory")]
    MissingHome,
    #[error("could not resolve the current user's {0} directory")]
    MissingPlatformDirectory(&'static str),
    #[error("Claude Desktop state path is invalid")]
    InvalidStatePath,
    #[error("Claude Desktop managed state contains an unsafe symbolic link")]
    UnsafeSymlink,
    #[error("could not create a configuration directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("could not protect private Claude Desktop state: {0}")]
    Permissions(std::io::Error),
    #[error("could not lock the Claude Desktop integration: {0}")]
    Lock(std::io::Error),
    #[error("could not check whether Claude Desktop is running: {0}")]
    ProcessCheck(std::io::Error),
    #[error("the Claude Desktop process check failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[error("could not launch Claude Desktop: {0}")]
    Launch(std::io::Error),
    #[error("Claude Desktop launcher failed with exit code {0:?}")]
    LaunchFailed(Option<i32>),
    #[error(
        "could not terminate Claude Desktop, so its configuration was not restored; quit it completely, then run `nan claude-desktop --restore`: {0}"
    )]
    Terminate(std::io::Error),
    #[error(
        "Claude Desktop termination failed with exit code {0:?}, so its configuration was not restored; quit it completely, then run `nan claude-desktop --restore`"
    )]
    TerminateFailed(Option<i32>),
    #[error("could not read Claude Desktop configuration: {0}")]
    ReadConfig(std::io::Error),
    #[error("Claude Desktop configuration is not valid JSON: {0}")]
    ParseConfig(serde_json::Error),
    #[error("Claude Desktop configuration root must be an object")]
    ConfigRoot,
    #[error("could not serialize Claude Desktop configuration: {0}")]
    SerializeConfig(serde_json::Error),
    #[error("could not write Claude Desktop configuration: {0}")]
    Write(std::io::Error),
    #[error("could not restore Claude Desktop configuration: {0}")]
    Restore(std::io::Error),
    #[error(
        "an orphaned Claude Desktop backup exists; inspect the private state directory before retrying"
    )]
    OrphanBackup,
    #[error("could not create the private Claude Desktop backup directory: {0}")]
    CreateBackupDirectory(std::io::Error),
    #[error("could not write a private Claude Desktop backup: {0}")]
    WriteBackup(std::io::Error),
    #[error("could not read a private Claude Desktop backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Claude Desktop backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Claude Desktop backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not serialize the private Claude Desktop receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("could not read the private Claude Desktop receipt: {0}")]
    ReadReceipt(std::io::Error),
    #[error("the private Claude Desktop receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the private Claude Desktop receipt schema is not supported")]
    UnsupportedReceipt,
    #[error("could not remove the restored Claude Desktop receipt: {0}")]
    RemoveReceipt(std::io::Error),
}

impl ClaudeDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(error) => error.code(),
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::OrphanReceipt
            | Self::OrphanBackup
            | Self::UnsafeSymlink => "NH-DESKTOP-002",
            Self::UnsupportedPlatform
            | Self::AppNotFound { .. }
            | Self::Compatibility(
                nan_harness_runtime::DesktopCompatibilityError::Unavailable
                | nan_harness_runtime::DesktopCompatibilityError::MissingPlatform,
            ) => "NH-DESKTOP-003",
            _ => "NH-DESKTOP-001",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn paths() -> (tempfile::TempDir, DesktopPaths) {
        let root = tempfile::tempdir().expect("temp root");
        let paths = DesktopPaths::new(
            &root.path().join("Claude"),
            &root.path().join("Claude-3p"),
            &root.path().join("state"),
        );
        (root, paths)
    }

    #[test]
    fn dry_run_plan_preserves_model_executable_diagnostics_and_search_policy() {
        let arguments = ClaudeDesktopArgs {
            model: Some("qwen3.6".to_owned()),
            provider_base_url: None,
            executable: Some(PathBuf::from("/tmp/claude")),
            allow_unsupported: false,
            allow_untested: false,
            search: crate::app::WebSearchArgs {
                no_search: false,
                force_search: true,
            },
            dry_run: true,
            show_auto: true,
            restore: false,
        };

        let plan = dry_run_plan(&arguments);

        assert_eq!(plan.harness, DesktopHarnessKind::Claude);
        assert_eq!(plan.transport, DesktopTransport::AnthropicBridge);
        assert_eq!(plan.executable, arguments.executable);
        assert_eq!(plan.selected_model, arguments.model);
        assert_eq!(plan.web_search_policy, WebSearchPolicy::Force);
        assert!(plan.private_diagnostics);
    }

    #[test]
    fn macos_paths_use_application_support_and_accept_a_nan_override() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/Users/tester")),
            nan_config: Some(PathBuf::from("/private/nan")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Macos, &environment)
            .expect("macOS paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from(
                "/Users/tester/Library/Application Support/Claude/claude_desktop_config.json"
            )
        );
        assert_eq!(
            paths.profile,
            PathBuf::from(
                "/Users/tester/Library/Application Support/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
            )
        );
        assert_eq!(
            paths.receipt,
            PathBuf::from("/private/nan/claude-desktop-receipt.json")
        );
    }

    #[test]
    fn linux_paths_follow_xdg_config_home() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/home/tester")),
            xdg_config: Some(PathBuf::from("/var/lib/tester/config")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
            .expect("Linux XDG paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("/var/lib/tester/config/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.third_party_config,
            PathBuf::from("/var/lib/tester/config/Claude-3p/claude_desktop_config.json")
        );
        assert_eq!(
            paths.lock,
            PathBuf::from("/var/lib/tester/config/nan-harness/claude-desktop.lock")
        );
    }

    #[test]
    fn linux_paths_fall_back_to_the_home_config_directory() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/home/tester")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
            .expect("Linux home paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("/home/tester/.config/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.profile,
            PathBuf::from(
                "/home/tester/.config/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
            )
        );
    }

    #[test]
    fn windows_paths_separate_roaming_standard_state_from_local_third_party_state() {
        let environment = DesktopEnvironment {
            app_data: Some(PathBuf::from("roaming")),
            local_app_data: Some(PathBuf::from("local")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Windows, &environment)
            .expect("Windows paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("roaming/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.third_party_config,
            PathBuf::from("local/Claude-3p/claude_desktop_config.json")
        );
        assert_eq!(
            paths.receipt,
            PathBuf::from("roaming/nan-harness/claude-desktop-receipt.json")
        );
    }

    #[test]
    fn windows_tasklist_detection_ignores_localized_empty_output() {
        assert!(!tasklist_reports_desktop(
            b"INFO: No tasks are running which match the specified criteria.\r\n"
        ));
        assert!(tasklist_reports_desktop(
            b"\"Claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
        ));
        assert!(tasklist_reports_desktop(
            b"\"claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
        ));
    }

    #[test]
    fn auto_mode_activity_renders_the_provider_request() {
        let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReview {
            review_id: 7,
            stage: ClaudeAutoModeReviewStage::Initial,
            model_id: "qwen3.6".to_owned(),
            request: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
                r#"{"model":"qwen3.6","temperature":0}"#,
            ),
        });

        assert_eq!(
            message,
            concat!(
                "[Auto #7] Claude requested a permission review (stage 1, classifier qwen3.6).\n",
                "[Auto #7] NaN request:\n",
                "{\n  \"model\": \"qwen3.6\",\n  \"temperature\": 0\n}"
            )
        );
    }

    #[test]
    fn auto_mode_response_pretty_prints_json_and_preserves_non_json_bodies() {
        let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id: 7,
            status: 200,
            response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
                r#"{"choices":[{"message":{"content":"reviewed"}}]}"#,
            ),
        });
        assert!(response.contains("[Auto #7] NaN response (HTTP 200):"));
        assert!(response.contains("\"content\": \"reviewed\""));

        let plain_text = "provider response body\n";
        let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id: 8,
            status: 200,
            response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(plain_text),
        });
        assert!(response.ends_with(plain_text));
    }

    #[test]
    fn auto_mode_failure_is_correlated_without_transport_details() {
        let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewFailed {
            review_id: 9,
            error_code: "NH-BRIDGE-103",
        });

        assert_eq!(
            message,
            "[Auto #9] NaN request failed before a response was received (NH-BRIDGE-103)."
        );
    }

    #[test]
    fn launch_message_mentions_auto_only_when_tracing_is_enabled() {
        assert_eq!(
            launch_message(false),
            "Claude Desktop launched through NaN."
        );
        assert!(!launch_message(false).contains("Auto"));
        assert!(launch_message(true).contains("Auto traces will appear here"));
        assert!(launch_message(true).contains("private data"));
    }

    #[test]
    fn apply_preserves_unknown_fields_and_restore_is_exact() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent")).expect("dir");
        fs::write(
            &paths.normal_config,
            b"{\"unknown\":{\"kept\":true},\"deploymentMode\":\"1p\"}\n",
        )
        .expect("original");
        let original = fs::read(&paths.normal_config).expect("read original");
        let receipt = Receipt::capture(&paths).expect("capture");
        receipt.write(&paths.receipt).expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-only").expect("apply");
        let active: Value =
            serde_json::from_slice(&fs::read(&paths.normal_config).expect("read active"))
                .expect("json");
        assert_eq!(active["unknown"]["kept"], true);
        let active_profile: Value =
            serde_json::from_slice(&fs::read(&paths.profile).expect("read active profile"))
                .expect("profile json");
        assert_eq!(active_profile["modelDiscoveryEnabled"], true);
        assert_eq!(active_profile["autoModeEnabled"], true);
        restore_receipt(&paths).expect("restore");
        assert_eq!(
            fs::read(&paths.normal_config).expect("read restored"),
            original
        );
        assert!(!paths.profile.exists());
    }

    #[test]
    fn receipt_json_never_contains_backed_up_config_or_provider_key() {
        let (_root, paths) = paths();
        let provider_key = "real-provider-secret";
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        fs::write(
            &paths.profile,
            format!(r#"{{"inferenceGatewayApiKey":"{provider_key}","unknown":true}}"#),
        )
        .expect("original profile");
        let receipt = Receipt::capture(&paths).expect("capture");
        receipt.write(&paths.receipt).expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let receipt_text = fs::read_to_string(&paths.receipt).expect("receipt text");
        assert!(
            !receipt_text.contains(provider_key),
            "receipt metadata copied original configuration contents"
        );
        assert!(!receipt_text.contains("inferenceGatewayApiKey"));
        assert!(!receipt_text.contains("session-token"));
        assert!(
            !fs::read_to_string(&paths.profile)
                .expect("profile text")
                .contains(provider_key)
        );
        assert!(
            fs::read_to_string(&paths.profile)
                .expect("profile text")
                .contains("session-token")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let backup = paths.backup_directory.join("document-3.backup");
            assert_eq!(
                fs::metadata(backup)
                    .expect("backup metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn stale_receipt_recovers_all_documents() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.meta.parent().expect("parent")).expect("dir");
        fs::write(&paths.meta, b"{\"before\":1}").expect("original");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        fs::write(&paths.meta, b"{\"after\":2}").expect("changed");
        restore_receipt(&paths).expect("restore");
        assert_eq!(fs::read(&paths.meta).expect("restored"), b"{\"before\":1}");
        assert!(!paths.receipt.exists());
    }

    #[test]
    fn normal_start_rejects_orphan_backup_without_deleting_it() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.backup_directory).expect("backup directory");
        let sentinel = paths.backup_directory.join("inspect-me.backup");
        fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

        let error = ensure_no_pending_recovery(&paths).expect_err("orphan should block startup");

        assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
        assert_eq!(
            fs::read(sentinel).expect("orphan backup should remain"),
            b"recoverable configuration"
        );
    }

    #[test]
    fn restore_reports_orphan_backup_when_receipt_is_missing() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.backup_directory).expect("backup directory");
        let sentinel = paths.backup_directory.join("inspect-me.backup");
        fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

        let error = restore_receipt(&paths).expect_err("orphan should require inspection");

        assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
        assert!(sentinel.exists(), "orphan backup should remain recoverable");
    }

    #[test]
    fn session_lock_rejects_concurrency() {
        let (_root, paths) = paths();
        let _first = SessionLock::acquire(&paths.lock).expect("first lock");
        assert!(matches!(
            SessionLock::acquire(&paths.lock),
            Err(ClaudeDesktopError::ConcurrentSession)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn configuration_symlinks_are_rejected_without_touching_the_target() {
        use std::os::unix::fs::symlink;

        let (_root, paths) = paths();
        let target = paths
            .normal_config
            .parent()
            .expect("normal parent")
            .join("user-owned.json");
        fs::create_dir_all(target.parent().expect("target parent")).expect("target directory");
        fs::write(&target, b"{\"private\":true}").expect("target contents");
        symlink(&target, &paths.normal_config).expect("configuration symlink");

        let error = Receipt::capture(&paths).expect_err("symlink must be rejected");

        assert!(matches!(error, ClaudeDesktopError::UnsafeSymlink));
        assert_eq!(
            fs::read(&target).expect("target should remain readable"),
            b"{\"private\":true}"
        );
        assert!(
            fs::symlink_metadata(&paths.normal_config)
                .expect("symlink should remain")
                .file_type()
                .is_symlink()
        );
        assert!(!paths.backup_directory.exists());
    }

    struct FakeProcess {
        profile: PathBuf,
        available: AtomicBool,
        running: AtomicBool,
        terminated: AtomicBool,
        force_terminated: AtomicBool,
        terminated_while_gateway_active: AtomicBool,
        fail_checks: AtomicBool,
        transient_check_failures: AtomicUsize,
        fail_terminate: AtomicBool,
        fail_force_terminate: AtomicBool,
    }

    impl FakeProcess {
        fn running(profile: PathBuf) -> Self {
            Self {
                profile,
                available: AtomicBool::new(true),
                running: AtomicBool::new(true),
                terminated: AtomicBool::new(false),
                force_terminated: AtomicBool::new(false),
                terminated_while_gateway_active: AtomicBool::new(false),
                fail_checks: AtomicBool::new(false),
                transient_check_failures: AtomicUsize::new(0),
                fail_terminate: AtomicBool::new(false),
                fail_force_terminate: AtomicBool::new(false),
            }
        }
    }

    impl DesktopProcess for FakeProcess {
        fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
            if self.available.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ClaudeDesktopError::AppNotFound { platform: "test" })
            }
        }

        fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
            let transient_failure = self
                .transient_check_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok();
            if self.fail_checks.load(Ordering::SeqCst) || transient_failure {
                return Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                    "synthetic process check failure",
                )));
            }
            Ok(self.running.load(Ordering::SeqCst))
        }

        fn launch(&self) -> Result<(), ClaudeDesktopError> {
            self.running.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn terminate(&self) -> Result<(), ClaudeDesktopError> {
            if self.fail_terminate.load(Ordering::SeqCst) {
                return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                    "synthetic termination failure",
                )));
            }
            let gateway_active = read_json_object(&self.profile).is_ok_and(|profile| {
                profile.get("inferenceProvider").and_then(Value::as_str) == Some("gateway")
            });
            self.terminated_while_gateway_active
                .store(gateway_active, Ordering::SeqCst);
            self.terminated.store(true, Ordering::SeqCst);
            self.running.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
            self.force_terminated.store(true, Ordering::SeqCst);
            if self.fail_force_terminate.load(Ordering::SeqCst) {
                return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                    "synthetic forced termination failure",
                )));
            }
            self.terminated.store(true, Ordering::SeqCst);
            self.running.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn missing_desktop_is_rejected_before_session_state_setup() {
        let (_root, paths) = paths();
        let process = FakeProcess::running(paths.profile.clone());
        process.available.store(false, Ordering::SeqCst);

        assert!(matches!(
            prepare_session_lock(&paths, &process),
            Err(ClaudeDesktopError::AppNotFound { .. })
        ));
        assert!(!paths.lock.exists());
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn signal_terminates_desktop_before_exact_restore() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"original\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());

        let exit_code = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(130)))
            .await
            .expect("signal cleanup");

        assert_eq!(exit_code, 130);
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(
            process
                .terminated_while_gateway_active
                .load(Ordering::SeqCst),
            "profile was restored before Claude Desktop was terminated"
        );
        assert_eq!(
            fs::read(&paths.profile).expect("restored profile"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn process_wait_error_still_restores_exact_configuration() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent"))
            .expect("config directory");
        let original = b"{\"deploymentMode\":\"1p\",\"kept\":7}\n";
        fs::write(&paths.normal_config, original).expect("original config");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());
        process.transient_check_failures.store(1, Ordering::SeqCst);
        let wait_error = wait_for_exit_or_signal(&process).await;

        let error = complete_and_restore(&paths, &process, wait_error)
            .await
            .expect_err("process error should propagate");

        assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.normal_config).expect("restored config"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[test]
    fn apply_error_restores_before_launch() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent"))
            .expect("config directory");
        let original = b"{\"deploymentMode\":\"1p\",\"kept\":8}\n";
        fs::write(&paths.normal_config, original).expect("original config");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");

        let error = restore_after(&paths, Err(ClaudeDesktopError::ConfigRoot))
            .expect_err("apply error should propagate");

        assert!(matches!(error, ClaudeDesktopError::ConfigRoot));
        assert_eq!(
            fs::read(&paths.normal_config).expect("restored config"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn launch_error_terminates_partial_launch_before_restore() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"before-launch\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());

        let error = complete_and_restore(
            &paths,
            &process,
            Err(ClaudeDesktopError::LaunchFailed(Some(1))),
        )
        .await
        .expect_err("launch error should propagate");

        assert!(matches!(error, ClaudeDesktopError::LaunchFailed(Some(1))));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(
            process
                .terminated_while_gateway_active
                .load(Ordering::SeqCst)
        );
        assert_eq!(
            fs::read(&paths.profile).expect("restored profile"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn termination_failure_leaves_active_config_and_recovery_state() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"original\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let active = fs::read(&paths.profile).expect("active profile");
        let process = FakeProcess::running(paths.profile.clone());
        process.fail_terminate.store(true, Ordering::SeqCst);
        process.fail_force_terminate.store(true, Ordering::SeqCst);

        let error = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(143)))
            .await
            .expect_err("unsafe cleanup should fail");

        assert!(matches!(error, ClaudeDesktopError::Terminate(_)));
        assert!(process.force_terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.profile).expect("profile should remain active"),
            active
        );
        assert!(paths.receipt.exists(), "receipt should remain recoverable");
        assert!(
            paths.backup_directory.exists(),
            "backup should remain recoverable"
        );
    }

    #[tokio::test]
    async fn persistent_process_check_error_does_not_restore_without_confirmation() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        fs::write(&paths.profile, b"{\"userField\":\"original\"}\n").expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let active = fs::read(&paths.profile).expect("active profile");
        let process = FakeProcess::running(paths.profile.clone());
        process.fail_checks.store(true, Ordering::SeqCst);

        let error = complete_and_restore(
            &paths,
            &process,
            Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                "synthetic wait failure",
            ))),
        )
        .await
        .expect_err("unconfirmed termination should fail");

        assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(process.force_terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.profile).expect("profile should remain active"),
            active
        );
        assert!(paths.receipt.exists(), "receipt should remain recoverable");
        assert!(
            paths.backup_directory.exists(),
            "backup should remain recoverable"
        );
    }
}
