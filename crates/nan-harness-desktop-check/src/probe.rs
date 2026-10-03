//! Each native accessibility probe runs in a bounded child process.

use crate::{
    gui::{ComposerFailure, Gui, GuiFailure},
    process::ProbeProcess,
    provider::ProviderGate,
    report::{CheckStep, InputMode, ProbeResult, Reason, Status},
};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::{create_private_dir_all, open_private_new};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt as _, process::Command};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProbeSpec {
    pub(crate) kind: DesktopHarnessKind,
    pub(crate) nan_harness: PathBuf,
    pub(crate) nan_harness_sha256: String,
    pub(crate) executable: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) model: String,
    pub(crate) live: bool,
    pub(crate) probe_index: Option<usize>,
    #[serde(default)]
    pub(crate) session: crate::cli::SessionMode,
    #[serde(default)]
    pub(crate) verification: crate::cli::VerificationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_wrapper: Option<LaunchWrapper>,
}

#[cfg(all(test, windows))]
mod windows_process_tests {
    use super::*;

    pub(super) async fn descendant_alive(pid: u32) -> bool {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &format!(
            "try {{ $p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($null -eq $p) {{ exit 41 }}; exit 0 }} catch {{ exit 42 }}"
        )]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
        let mut query = command.spawn().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), query.wait())
            .await
            .unwrap()
            .unwrap();
        match status.code() {
            Some(0) => true,
            Some(41) => false,
            _ => panic!("synthetic process observation failed"),
        }
    }

    #[tokio::test]
    async fn stop_terminates_a_job_owned_parent_and_descendant_without_touching_sentinel() {
        let ready = tempfile::tempdir().unwrap();
        let ready_path = ready.path().join("ready");
        let mut command = Command::new("powershell.exe");
        command.env("NANH_JOB_READY", &ready_path);
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference = 'Stop'; $child = Start-Process powershell.exe -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 120' -PassThru; Set-Content -Encoding Ascii $env:NANH_JOB_READY $child.Id",
        ]);
        let mut process = ProbeProcess::spawn(command).unwrap();
        let mut sentinel = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 120",
            ])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let descendant = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(&ready_path)
                    && let Ok(pid) = pid.trim().parse::<u32>()
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(10), process.wait_launcher())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert!(descendant_alive(descendant).await);
        assert_eq!(stop(&mut process, None, None).await, Ok(()));
        assert!(sentinel.try_wait().unwrap().is_none());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if !descendant_alive(descendant).await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let _ = sentinel.kill().await;
        drop(ready);
    }
}

mod claude_native_roots;
mod claude_native_storage;
mod claude_storage;
mod hermes_policy;
pub(crate) mod hermes_readiness;
mod semantic;

/// Opt-in startup diagnostic binding. Only the `chatgpt-desktop` launch runs
/// through the wrapper; `nan_harness` and its digest remain the tested
/// identity, and the wrapper's closed facts stay beside the report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LaunchWrapper {
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
    /// A fresh owner-only directory for this probe's facts alone.
    pub(crate) facts: PathBuf,
}

/// The wrapper stops its own observation at this deadline. It must outlast
/// every worker bound, so the checker's stop and timeout keep governing.
const LAUNCH_WRAPPER_DEADLINE_SECONDS: u64 = 300;
const PROCESS_OBSERVATION_ENV_PATH: &str = "NAN_NATIVE_PROCESS_OBSERVATION";
// The wrapper's reducer refuses a deadline above ten minutes.
const _: () = assert!(LAUNCH_WRAPPER_DEADLINE_SECONDS <= 600);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkerOutcome {
    pub(crate) result: ProbeResult,
    pub(crate) launch_exit: Option<LaunchExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) child_exit: Option<LaunchExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovery_exit: Option<LaunchExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_failure: Option<crate::diagnostics::LaunchFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) setup_cause: Option<crate::diagnostics::SetupCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovery_cause: Option<crate::diagnostics::DiscoveryCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) startup: Option<crate::diagnostics::StartupDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) native_process_observation: Option<crate::diagnostics::NativeProcessObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_identity_observation: Option<crate::diagnostics::ClaudeIdentityObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matched_window_inventory: Option<crate::diagnostics::ClaudeMatchedWindowInventory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_readiness: Option<crate::diagnostics::ClaudeReadiness>,
    pub(crate) cleanup: Option<CleanupDiagnostic>,
    #[serde(default)]
    pub(crate) composer: Vec<ComposerFailure>,
    #[serde(default)]
    pub(crate) gui_acquisition: Option<crate::diagnostics::GuiAcquisitionDiagnostic>,
}

impl WorkerOutcome {
    /// Reject diagnostic combinations that could not have been emitted by the
    /// native helper before accepting a worker result as authoritative.
    pub(crate) fn validate_diagnostics(&self) -> Result<(), ()> {
        if self.setup_cause.is_some()
            && self.launch_failure != Some(crate::diagnostics::LaunchFailure::LaunchSetup)
        {
            return Err(());
        }
        if self.discovery_cause.is_some()
            && (self.launch_failure != Some(crate::diagnostics::LaunchFailure::LaunchSetup)
                || self.setup_cause != Some(crate::diagnostics::SetupCause::Discovery))
        {
            return Err(());
        }
        if let Some(exit) = self.discovery_exit
            && (self.launch_failure != Some(crate::diagnostics::LaunchFailure::LaunchSetup)
                || self.setup_cause != Some(crate::diagnostics::SetupCause::Discovery)
                || self.discovery_cause
                    != Some(crate::diagnostics::DiscoveryCause::VersionCommandFailed)
                || !valid_discovery_exit(exit))
        {
            return Err(());
        }
        if self.child_exit.is_some()
            && (self.launch_failure != Some(crate::diagnostics::LaunchFailure::NativeAppExited)
                || self.child_exit.is_some_and(|exit| !valid_child_exit(exit)))
        {
            return Err(());
        }
        if let Some(acquisition) = self.gui_acquisition
            && let Some(relation) = acquisition.foreground_relation
        {
            if !cfg!(windows)
                || acquisition.stage != crate::diagnostics::GuiAcquisitionStage::WindowStability
            {
                return Err(());
            }
            let valid = match acquisition.error_category {
                crate::gui::ComposerErrorCategory::NativeHelperFitForegroundRead => {
                    relation == crate::native::FitForegroundRelation::IdentityUnavailable
                }
                crate::gui::ComposerErrorCategory::NativeHelperFitForegroundMismatch
                | crate::gui::ComposerErrorCategory::NativeHelperFitForegroundChanged => true,
                _ => false,
            };
            if !valid {
                return Err(());
            }
        }
        if self.claude_readiness.is_some() && self.claude_identity_observation.is_none() {
            return Err(());
        }
        if self.matched_window_inventory.is_some()
            && self.claude_identity_observation
                != Some(
                    crate::diagnostics::ClaudeIdentityObservation::MatchingProcessNoVisibleWindow,
                )
        {
            return Err(());
        }
        for failure in &self.composer {
            if let Some(relation) = failure.geometry_relation {
                if failure.operation != crate::gui::ComposerOperation::Guard
                    || failure.guard_context
                        != Some(crate::gui::ComposerGuardContext::Reacquisition)
                    || failure.error_category != crate::gui::ComposerErrorCategory::WindowOffDisplay
                {
                    return Err(());
                }
                let _ = relation;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComposerDiagnostic {
    pub(crate) schema_version: u8,
    pub(crate) observations: Vec<ComposerFailure>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum LaunchExit {
    Code(i32),
    // The serialized diagnostic vocabulary is platform-independent; only Unix
    // process observation can produce a signal from a local exit status.
    Signal(i32),
    Unknown,
}

fn valid_discovery_exit(exit: LaunchExit) -> bool {
    match exit {
        LaunchExit::Code(code) => code != 0,
        LaunchExit::Signal(signal) => (1..=127).contains(&signal),
        LaunchExit::Unknown => false,
    }
}

fn valid_child_exit(exit: LaunchExit) -> bool {
    match exit {
        LaunchExit::Code(code) => code != 0,
        LaunchExit::Signal(signal) => (1..=127).contains(&signal),
        LaunchExit::Unknown => false,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CleanupStage {
    Stop,
    AbsenceAfterStop,
    Restore,
    AbsenceAfterRestore,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CleanupDiagnostic {
    stage: CleanupStage,
    original_reason: Option<Reason>,
    reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    absence: Option<crate::gui::AbsenceStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stop: Option<StopDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restore: Option<RestoreFailure>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RestoreFailure {
    CommandCreation,
    ProcessIo,
    DeadlineExpired,
    NonzeroExit,
}

/// Closed observations of each bounded process-stop operation. These facts
/// deliberately contain no process identity or command details.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum StopWaitOutcome {
    NotAttempted,
    Reaped,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum StopKillOutcome {
    NotAttempted,
    Issued,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopDiagnostic {
    initial_wait: StopWaitDiagnostic,
    grace_wait: StopWaitDiagnostic,
    kill: StopKillDiagnostic,
    final_wait: StopWaitDiagnostic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopWaitDiagnostic {
    outcome: StopWaitOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    os_error: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopKillDiagnostic {
    outcome: StopKillOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    os_error: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StopFailure {
    diagnostic: StopDiagnostic,
}

pub(crate) async fn run_worker(spec: &Path, output: &Path) -> Result<i32, String> {
    let bytes = std::fs::read(spec).map_err(|_| "probe specification cannot be read")?;
    if bytes.len() > 16 * 1024 {
        return Err("probe specification is too large".into());
    }
    let spec: ProbeSpec =
        serde_json::from_slice(&bytes).map_err(|_| "invalid probe specification")?;
    let result = execute(&spec).await;
    let mut file = open_private_new(output).map_err(|_| "probe result cannot be created")?;
    serde_json::to_writer(&mut file, &result).map_err(|_| "probe result cannot be recorded")?;
    file.sync_all()
        .map_err(|_| "probe result cannot be saved")?;
    Ok(i32::from(result.result.status != Status::Passed))
}

// Desktop executables can exceed 256 MiB. Bound both identity input size and
// working memory without changing the digest used by prepared receipts.
const MAX_DIGESTED_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn binary_digest(path: &Path) -> Result<String, Reason> {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let metadata = std::fs::metadata(path).map_err(|_| Reason::InstallationUnreadable)?;
    if !metadata.is_file() {
        return Err(Reason::InstallationUnreadable);
    }
    if metadata.len() > MAX_DIGESTED_BYTES {
        return Err(Reason::InstallationUnreadable);
    }
    let mut file = std::fs::File::open(path).map_err(|_| Reason::InstallationUnreadable)?;
    // Check the opened object too: the path may have changed after inspection.
    let opened = file
        .metadata()
        .map_err(|_| Reason::InstallationUnreadable)?;
    if !opened.is_file() || opened.len() > MAX_DIGESTED_BYTES {
        return Err(Reason::InstallationUnreadable);
    }
    let mut hasher = sha2::Sha256::new();
    let mut chunk = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut chunk)
            .map_err(|_| Reason::InstallationUnreadable)?;
        if count == 0 {
            break;
        }
        hasher.update(&chunk[..count]);
        total += count as u64;
        // A file that grows past the declared size during hashing is
        // unreadable identity evidence, not a smaller file.
        if total > MAX_DIGESTED_BYTES {
            return Err(Reason::InstallationUnreadable);
        }
    }
    if total != opened.len() {
        return Err(Reason::InstallationUnreadable);
    }
    let digest = hasher.finalize();
    Ok(digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        }))
}

pub(crate) async fn recover_pending(journal: &mut crate::journal::Journal) -> Result<(), String> {
    for name in journal.pending_names() {
        let root = journal.root().join(&name);
        let path = root.join("spec.json");
        if !path
            .try_exists()
            .map_err(|_| "cannot inspect pending recovery")?
        {
            continue;
        }
        if !std::fs::symlink_metadata(&root).is_ok_and(|metadata| metadata.file_type().is_dir())
            || !std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err("recovery paths changed; files retained".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| "cannot read pending recovery")?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "cannot read pending recovery")?;
        if bytes.len() > 16 * 1024 {
            return Err("invalid recovery specification".into());
        }
        let spec: ProbeSpec =
            serde_json::from_slice(&bytes).map_err(|_| "invalid recovery specification")?;
        if spec.workspace != root.join("workspace")
            || binary_digest(&spec.nan_harness).ok().as_deref()
                != Some(spec.nan_harness_sha256.as_str())
        {
            return Err("recovery identity changed; files retained".into());
        }
        Gui::ensure_absent(spec.kind)
            .map_err(|_| "close the tested application before recovery; files retained")?;
        restore(&spec)
            .await
            .map_err(|_| "native restoration needs attention; recovery files retained")?;
        journal
            .seal(&name)
            .map_err(|_| "cannot record recovered state")?;
    }
    Ok(())
}

#[derive(Default)]
struct LaunchObservation {
    exit: Option<LaunchExit>,
    child_exit: Option<LaunchExit>,
    discovery_exit: Option<LaunchExit>,
    failure: Option<crate::diagnostics::LaunchFailure>,
    setup_cause: Option<crate::diagnostics::SetupCause>,
    discovery_cause: Option<crate::diagnostics::DiscoveryCause>,
    startup: Option<crate::diagnostics::StartupDiagnostic>,
    native_process_observation: Option<crate::diagnostics::NativeProcessObservation>,
    claude_identity_observation: Option<crate::diagnostics::ClaudeIdentityObservation>,
    claude_readiness: Option<crate::diagnostics::ClaudeReadiness>,
    matched_window_inventory: Option<crate::diagnostics::ClaudeMatchedWindowInventory>,
}

impl LaunchObservation {
    fn capture(process: &mut ProbeProcess, spec: &ProbeSpec, failed_acquisition: bool) -> Self {
        let record = read_child_launch_diagnostic(spec);
        let claude_identity = read_claude_identity_observation(spec, failed_acquisition);
        Self {
            exit: process.try_wait().ok().flatten().map(launcher_exit),
            child_exit: record.as_ref().and_then(|record| record.child_exit),
            discovery_exit: record.as_ref().and_then(|record| record.discovery_exit),
            failure: record.as_ref().map(|record| record.failure),
            setup_cause: record.as_ref().and_then(|record| record.setup_cause),
            discovery_cause: record.as_ref().and_then(|record| record.discovery_cause),
            startup: record.and_then(|record| record.startup()),
            native_process_observation: read_native_process_observation(spec),
            claude_identity_observation: claude_identity.map(|value| value.0),
            claude_readiness: claude_identity.and_then(|value| value.1),
            matched_window_inventory: claude_identity.and_then(|value| value.2),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn identity_capture_allowed(spec: &ProbeSpec, failed_acquisition: bool) -> bool {
    failed_acquisition && cfg!(target_os = "macos") && spec.kind == DesktopHarnessKind::Claude
}

#[cfg(target_os = "macos")]
fn read_claude_identity_observation(
    spec: &ProbeSpec,
    failed_acquisition: bool,
) -> Option<(
    crate::diagnostics::ClaudeIdentityObservation,
    Option<crate::diagnostics::ClaudeReadiness>,
    Option<crate::diagnostics::ClaudeMatchedWindowInventory>,
)> {
    if !identity_capture_allowed(spec, failed_acquisition) {
        return None;
    }
    let unavailable = crate::diagnostics::ClaudeIdentityObservation::QueryUnavailable;
    let Some(executable) = std::fs::canonicalize(&spec.executable).ok() else {
        return Some((unavailable, None, None));
    };
    let Some(bundle) = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
    else {
        return Some((unavailable, None, None));
    };
    let Some(native) = crate::native::Native::new().ok() else {
        return Some((unavailable, None, None));
    };
    Some(native.claude_identity_observation(bundle).map_or(
        (unavailable, None, None),
        |(observation, readiness, inventory)| (observation, Some(readiness), inventory),
    ))
}

#[cfg(not(target_os = "macos"))]
fn read_claude_identity_observation(
    _spec: &ProbeSpec,
    _failed_acquisition: bool,
) -> Option<(
    crate::diagnostics::ClaudeIdentityObservation,
    Option<crate::diagnostics::ClaudeReadiness>,
    Option<crate::diagnostics::ClaudeMatchedWindowInventory>,
)> {
    None
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeProcessObservationRecord {
    schema_version: u8,
    observation: crate::diagnostics::NativeProcessObservationState,
    ever_observed_present: bool,
}

fn read_native_process_observation(
    spec: &ProbeSpec,
) -> Option<crate::diagnostics::NativeProcessObservation> {
    if spec.kind != DesktopHarnessKind::Claude || !cfg!(target_os = "macos") {
        return None;
    }
    let path = spec.workspace.join("native-process-observation.json");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > 512 {
        return None;
    }
    let file = nan_harness_private_fs::open_private_read(&path).ok()?.0;
    let mut bytes = Vec::new();
    file.take(513).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 512 {
        return None;
    }
    decode_native_process_observation(&bytes)
}

fn decode_native_process_observation(
    bytes: &[u8],
) -> Option<crate::diagnostics::NativeProcessObservation> {
    if bytes.len() > 512 {
        return None;
    }
    let record = serde_json::from_slice::<NativeProcessObservationRecord>(bytes).ok()?;
    if matches!(
        record.observation,
        crate::diagnostics::NativeProcessObservationState::MatchingProcessPresent
    ) && !record.ever_observed_present
    {
        return None;
    }
    (record.schema_version == 1).then_some(crate::diagnostics::NativeProcessObservation {
        state: record.observation,
        ever_observed_present: record.ever_observed_present,
    })
}

async fn execute(spec: &ProbeSpec) -> WorkerOutcome {
    let started = Instant::now();
    let mut result = ProbeResult::blocked(Reason::NotRun);
    let mut cleanup = None;
    let mut launch_observation = LaunchObservation::default();
    let mut composer_observations = Vec::new();
    let mut diagnostic_allowed = false;
    let mut gui_acquisition = None;
    let outcome = scenario(
        spec,
        &mut result,
        &mut launch_observation,
        &mut cleanup,
        &mut composer_observations,
        &mut diagnostic_allowed,
        &mut gui_acquisition,
    )
    .await;
    if diagnostic_allowed {
        let wrapper = spec
            .launch_wrapper
            .as_ref()
            .expect("diagnostic permission requires a wrapper");
        let diagnostic = ComposerDiagnostic {
            schema_version: 1,
            observations: composer_observations.clone(),
        };
        if let Ok(mut file) = open_private_new(&wrapper.facts.join("composer-diagnostic.json")) {
            let _ = serde_json::to_writer(&mut file, &diagnostic);
            let _ = file.sync_all();
        }
    }
    match outcome {
        Ok(()) => {
            result.status = Status::Passed;
            result.reason = None;
        }
        Err(reason) => {
            result.reason = Some(reason);
            result.status = if matches!(
                reason,
                Reason::AlreadyRunning
                    | Reason::PermissionRequired
                    | Reason::LoginRequired
                    | Reason::IsolationUnavailable
                    | Reason::FocusChanged
                    | Reason::WindowChanged
                    | Reason::WindowOccluded
                    | Reason::DesktopUnavailable
            ) {
                Status::Blocked
            } else {
                Status::Failed
            };
        }
    }
    result.duration_milliseconds = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    WorkerOutcome {
        result,
        launch_exit: launch_observation.exit,
        child_exit: launch_observation.child_exit,
        discovery_exit: launch_observation.discovery_exit,
        launch_failure: launch_observation.failure,
        setup_cause: launch_observation.setup_cause,
        discovery_cause: launch_observation.discovery_cause,
        startup: launch_observation.startup,
        native_process_observation: launch_observation.native_process_observation,
        claude_identity_observation: launch_observation.claude_identity_observation,
        claude_readiness: launch_observation.claude_readiness,
        matched_window_inventory: launch_observation.matched_window_inventory,
        cleanup,
        composer: composer_observations,
        gui_acquisition,
    }
}

async fn scenario(
    spec: &ProbeSpec,
    result: &mut ProbeResult,
    launch_observation: &mut LaunchObservation,
    diagnostic: &mut Option<CleanupDiagnostic>,
    composer_observations: &mut Vec<ComposerFailure>,
    diagnostic_allowed: &mut bool,
    gui_acquisition: &mut Option<crate::diagnostics::GuiAcquisitionDiagnostic>,
) -> Result<(), Reason> {
    *diagnostic_allowed = validate_launch_binding(spec)?;
    prepare_scenario(spec).await?;
    let mut native_roots = claude_native_roots::NativeRoots::prepare(spec).await?;
    let outcome = scenario_owned(
        spec,
        result,
        launch_observation,
        diagnostic,
        composer_observations,
        gui_acquisition,
    )
    .await;
    if let Some(roots) = &mut native_roots {
        roots.cleanup()?;
    }
    outcome
}

async fn scenario_owned(
    spec: &ProbeSpec,
    result: &mut ProbeResult,
    launch_observation: &mut LaunchObservation,
    diagnostic: &mut Option<CleanupDiagnostic>,
    composer_observations: &mut Vec<ComposerFailure>,
    gui_acquisition: &mut Option<crate::diagnostics::GuiAcquisitionDiagnostic>,
) -> Result<(), Reason> {
    let experiment = HostedExperiment::from_spec(spec)?;
    let semantic = semantic::SemanticBackend::from_spec(spec)?;
    if semantic.is_some() && experiment.is_some() {
        return Err(Reason::IsolationUnavailable);
    }
    let marker = visual_marker("NAN CHECK READ")?;
    let fixture = prepare_read_fixture(spec, &marker)?;
    let final_marker = visual_marker("NAN CHECK RESPONSE")?;
    let inventory = ScriptedProvider::start(ProviderScenario::inventory(&final_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    let gate = start_provider_gate(spec, &inventory, &marker).await?;
    if experiment.is_some() {
        gate.expect_fixture_response(&final_marker)
            .map_err(|()| Reason::IsolationUnavailable)?;
    }
    let mut process = launch(spec, &gate).map_err(|(reason, failure)| {
        launch_observation.failure = Some(failure);
        reason
    })?;
    #[cfg(unix)]
    let process_group = process.id().and_then(|pid| i32::try_from(pid).ok());
    #[cfg(windows)]
    let process_group = None;
    let conversation = ConversationScenario {
        spec,
        inventory: &inventory,
        gate: &gate,
        fixture: &fixture,
        marker: &marker,
        final_marker: &final_marker,
        experiment: experiment.as_ref(),
        semantic: semantic.as_ref(),
    };
    let mut gui = None;
    let outcome = if conversation.uses_renderer() {
        conversation.run_renderer(&mut process, result).await
    } else {
        let acquired = Gui::wait(spec.kind, &mut process);
        capture_failed_acquisition(acquired.is_err(), &mut process, spec, launch_observation);
        #[cfg(any(target_os = "macos", windows))]
        let acquired = acquired.and_then(|native_gui| {
            native_gui.finish_initial_ready(&mut process)?;
            Ok(native_gui)
        });
        match acquired {
            Ok(native_gui) => {
                result.steps.push(CheckStep::Launched);
                let outcome = conversation
                    .run(&native_gui, process.id(), result, composer_observations)
                    .await;
                gui = Some(native_gui);
                outcome
            }
            Err((
                reason,
                acquisition_stage,
                error_category,
                foreground_relation,
                candidate_facts,
            )) => {
                *gui_acquisition = Some(crate::diagnostics::GuiAcquisitionDiagnostic {
                    stage: acquisition_stage,
                    error_category,
                    reason,
                    foreground_relation,
                    candidate_facts,
                });
                Err(reason)
            }
        }
    };
    if matches!(outcome, Err(Reason::WindowChanged | Reason::FocusChanged))
        && cfg!(target_os = "macos")
        && spec.kind == DesktopHarnessKind::Claude
        && std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref() == Ok("native-known-folders")
        && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline")
    {
        capture_failed_acquisition(true, &mut process, spec, launch_observation);
    }
    finish_scenario(
        spec,
        &mut process,
        gui.as_ref(),
        process_group,
        outcome,
        &gate,
        diagnostic,
    )
    .await
}

async fn prepare_scenario(spec: &ProbeSpec) -> Result<(), Reason> {
    // Windows known folders and credential stores follow the OS identity, not
    // HOME. Only an explicitly declared disposable hosted VM may use that account.
    if !spec.session.available()
        || (cfg!(windows) && spec.session != crate::cli::SessionMode::GithubHosted)
    {
        return Err(Reason::IsolationUnavailable);
    }
    Gui::ensure_absent_before_launch(spec.kind).map_err(|failure| failure.reason)?;
    require_endpoint_override(spec).await?;
    create_private_dir_all(&spec.workspace).map_err(|_| Reason::IsolationUnavailable)?;
    prepare_zed_profile(spec)?;
    Ok(())
}

fn validate_launch_binding(spec: &ProbeSpec) -> Result<bool, Reason> {
    if binary_digest(&spec.nan_harness)? != spec.nan_harness_sha256 {
        return Err(Reason::InstallationUnreadable);
    }
    if let Some(wrapper) = &spec.launch_wrapper {
        prepare_launch_wrapper(spec.kind, wrapper)?;
    }
    Ok(spec.launch_wrapper.is_some())
}

fn prepare_read_fixture(spec: &ProbeSpec, marker: &str) -> Result<PathBuf, Reason> {
    let fixture = spec.workspace.join("read-target.txt");
    open_private_new(&fixture)
        .and_then(|mut file| file.write_all(marker.as_bytes()))
        .map_err(|_| Reason::IsolationUnavailable)?;
    Ok(fixture)
}

// Keep conversation adapters separate from process acquisition and restoration.
struct ConversationScenario<'a> {
    spec: &'a ProbeSpec,
    inventory: &'a ScriptedProvider,
    gate: &'a ProviderGate,
    fixture: &'a Path,
    marker: &'a str,
    final_marker: &'a str,
    experiment: Option<&'a HostedExperiment>,
    semantic: Option<&'a semantic::SemanticBackend>,
}

impl ConversationScenario<'_> {
    fn uses_renderer(&self) -> bool {
        self.semantic
            .is_some_and(semantic::SemanticBackend::uses_renderer)
    }

    async fn run_renderer(
        &self,
        process: &mut ProbeProcess,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        self.semantic
            .ok_or(Reason::IsolationUnavailable)?
            .run_renderer(
                process,
                semantic::SemanticScenario {
                    inventory: self.inventory,
                    gate: self.gate,
                    fixture: self.fixture,
                    marker: self.final_marker,
                },
                result,
            )
            .await
    }

    async fn run(
        &self,
        gui: &Gui,
        owner: Option<u32>,
        result: &mut ProbeResult,
        composer_observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        if let Some(experiment) = self.experiment {
            // Partial hosted experiments cannot become compatibility evidence.
            return experiment
                .run(
                    gui,
                    owner,
                    self.final_marker,
                    result,
                    self.gate,
                    composer_observations,
                )
                .and(Err(Reason::NotRun));
        }
        if let Some(semantic) = self.semantic {
            return semantic
                .run(
                    gui,
                    owner,
                    semantic::SemanticScenario {
                        inventory: self.inventory,
                        gate: self.gate,
                        fixture: self.fixture,
                        marker: self.final_marker,
                    },
                    result,
                    composer_observations,
                )
                .await;
        }
        if let Err(failure) = gui.prepare_conversation() {
            result.gui_stage = Some(failure.stage);
            return Err(failure.reason);
        }
        if self.spec.live {
            live(
                gui,
                self.spec,
                self.gate,
                self.fixture,
                self.marker,
                result,
                composer_observations,
            )
        } else {
            deterministic(
                gui,
                self.inventory,
                self.gate,
                self.fixture,
                self.final_marker,
                result,
                composer_observations,
            )
            .await
        }
    }
}

async fn start_provider_gate(
    spec: &ProbeSpec,
    inventory: &ScriptedProvider,
    marker: &str,
) -> Result<ProviderGate, Reason> {
    let key = if spec.live {
        Zeroizing::new(std::env::var("NAN_API_KEY").map_err(|_| Reason::MissingKey)?)
    } else {
        Zeroizing::new("nanh-desktop-check-synthetic".into())
    };
    let upstream = if spec.live {
        "https://api.nan.builders/v1"
    } else {
        inventory.base_url()
    };
    ProviderGate::start(upstream, key, spec.live, marker)
        .await
        .map_err(|()| Reason::ProviderFailed)
}

enum HostedExperiment {
    Accessibility(PathBuf),
    NativeCopy(PathBuf),
    HermesDom(PathBuf),
    HermesStartup,
}

impl HostedExperiment {
    fn from_spec(spec: &ProbeSpec) -> Result<Option<Self>, Reason> {
        let mut selected = None;
        for candidate in [
            strict_accessibility_directory(spec)?.map(Self::Accessibility),
            native_copy_directory(spec)?.map(Self::NativeCopy),
            hermes_dom_directory(spec)?.map(Self::HermesDom),
            hermes_startup_opt_in(spec)?.map(|()| Self::HermesStartup),
        ]
        .into_iter()
        .flatten()
        {
            if selected.replace(candidate).is_some() {
                return Err(Reason::IsolationUnavailable);
            }
        }
        Ok(selected)
    }

    fn run(
        &self,
        gui: &Gui,
        owner: Option<u32>,
        marker: &str,
        result: &mut ProbeResult,
        gate: &ProviderGate,
        composer_observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        match self {
            Self::HermesStartup => gui.observe_hosted_startup(composer_observations),
            Self::Accessibility(directory) => {
                gui.probe_accessibility(directory, marker, result, gate)
            }
            Self::NativeCopy(directory) => gui.probe_native_copy(directory, marker, result, gate),
            Self::HermesDom(directory) => gui.probe_dom(
                directory,
                owner.ok_or(Reason::ApplicationExited)?,
                marker,
                result,
                gate,
            ),
        }
    }
}

fn hermes_startup_opt_in(spec: &ProbeSpec) -> Result<Option<()>, Reason> {
    let Some(value) = std::env::var_os("FEASIBILITY_HERMES_STARTUP_ONLY") else {
        return Ok(None);
    };
    if value != "1"
        || spec.live
        || spec.kind != DesktopHarnessKind::Hermes
        || !cfg!(target_os = "linux")
        || spec.session != crate::cli::SessionMode::GithubHosted
        || !spec.session.available()
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(Some(()))
}

fn hermes_dom_directory(spec: &ProbeSpec) -> Result<Option<PathBuf>, Reason> {
    let Some(directory) = std::env::var_os("FEASIBILITY_HERMES_DOM_FACTS") else {
        return Ok(None);
    };
    if spec.live
        || spec.kind != DesktopHarnessKind::Hermes
        || !cfg!(target_os = "linux")
        || spec.session != crate::cli::SessionMode::GithubHosted
        || !spec.session.available()
    {
        return Err(Reason::IsolationUnavailable);
    }
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err(Reason::IsolationUnavailable);
    }
    create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
    Ok(Some(directory))
}

fn strict_accessibility_directory(spec: &ProbeSpec) -> Result<Option<PathBuf>, Reason> {
    let Some(directory) = std::env::var_os("FEASIBILITY_ZED_AX_FACTS") else {
        return Ok(None);
    };
    if spec.live
        || spec.kind != DesktopHarnessKind::Zed
        || spec.session != crate::cli::SessionMode::GithubHosted
        || !spec.session.available()
    {
        return Err(Reason::IsolationUnavailable);
    }
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err(Reason::IsolationUnavailable);
    }
    create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
    Ok(Some(directory))
}

fn native_copy_directory(spec: &ProbeSpec) -> Result<Option<PathBuf>, Reason> {
    let Some(directory) = std::env::var_os("FEASIBILITY_ZED_NATIVE_COPY_FACTS") else {
        return Ok(None);
    };
    if spec.live
        || spec.kind != DesktopHarnessKind::Zed
        || spec.session != crate::cli::SessionMode::GithubHosted
        || !spec.session.available()
        || !cfg!(target_os = "macos")
        || std::env::var_os("FEASIBILITY_ZED_AX_FACTS").is_some()
    {
        return Err(Reason::IsolationUnavailable);
    }
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err(Reason::IsolationUnavailable);
    }
    create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
    Ok(Some(directory))
}

fn capture_failed_acquisition(
    failed: bool,
    process: &mut ProbeProcess,
    spec: &ProbeSpec,
    observation: &mut LaunchObservation,
) {
    if failed {
        // Capture the one opt-in native observation at failed acquisition,
        // before cleanup can alter process/window state.
        *observation = LaunchObservation::capture(process, spec, true);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChildLaunchDiagnostic {
    schema_version: u8,
    failure: crate::diagnostics::LaunchFailure,
    #[serde(default)]
    setup_cause: Option<crate::diagnostics::SetupCause>,
    #[serde(default)]
    discovery_cause: Option<crate::diagnostics::DiscoveryCause>,
    #[serde(default)]
    discovery_exit: Option<LaunchExit>,
    #[serde(default)]
    child_exit: Option<LaunchExit>,
    app_exit_code: Option<i32>,
    app_exit_signal: Option<i32>,
    startup_hint: Option<crate::diagnostics::StartupHint>,
    sandbox: Option<ChildSandboxDiagnostic>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChildSandboxDiagnostic {
    helper_presence: crate::diagnostics::SandboxHelperPresence,
    helper_mode: crate::diagnostics::SandboxHelperMode,
    helper_owner: crate::diagnostics::SandboxHelperOwner,
    helper_location: crate::diagnostics::SandboxHelperLocation,
    #[serde(rename = "apparmorUsernsRestriction")]
    apparmor_userns_restriction: crate::diagnostics::NamespacePolicy,
}

impl ChildLaunchDiagnostic {
    fn startup(&self) -> Option<crate::diagnostics::StartupDiagnostic> {
        Some(crate::diagnostics::StartupDiagnostic {
            exit: self
                .app_exit_code
                .map(LaunchExit::Code)
                .or_else(|| self.app_exit_signal.map(LaunchExit::Signal)),
            hint: self.startup_hint?,
            sandbox: self
                .sandbox
                .as_ref()
                .map(|facts| crate::diagnostics::SandboxFacts {
                    helper_presence: facts.helper_presence,
                    helper_mode: facts.helper_mode,
                    helper_owner: facts.helper_owner,
                    helper_location: facts.helper_location,
                    apparmor_userns_restriction: facts.apparmor_userns_restriction,
                }),
        })
    }
}

fn read_child_launch_diagnostic(spec: &ProbeSpec) -> Option<ChildLaunchDiagnostic> {
    let path = spec.workspace.join("native-launch-diagnostic.json");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > 1024 {
        return None;
    }
    let file = nan_harness_private_fs::open_private_read(&path).ok()?.0;
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    let record = serde_json::from_slice::<ChildLaunchDiagnostic>(&bytes).ok()?;
    if record.sandbox.is_some()
        && (!cfg!(target_os = "linux") || spec.kind != DesktopHarnessKind::ChatGpt)
    {
        return None;
    }
    if record.schema_version != 1
        || (record.app_exit_code.is_some() && record.app_exit_signal.is_some())
        || record
            .app_exit_signal
            .is_some_and(|signal| !(1..=127).contains(&signal))
    {
        return None;
    }
    if record.setup_cause.is_some()
        && record.failure != crate::diagnostics::LaunchFailure::LaunchSetup
    {
        return None;
    }
    if record.discovery_cause.is_some()
        && (record.failure != crate::diagnostics::LaunchFailure::LaunchSetup
            || record.setup_cause != Some(crate::diagnostics::SetupCause::Discovery))
    {
        return None;
    }
    if let Some(exit) = record.discovery_exit
        && (record.failure != crate::diagnostics::LaunchFailure::LaunchSetup
            || record.setup_cause != Some(crate::diagnostics::SetupCause::Discovery)
            || record.discovery_cause
                != Some(crate::diagnostics::DiscoveryCause::VersionCommandFailed)
            || !valid_discovery_exit(exit))
    {
        return None;
    }
    if record
        .child_exit
        .is_some_and(|exit| !valid_child_exit(exit))
        || record.child_exit.is_some()
            && (spec.kind != DesktopHarnessKind::Hermes
                || record.failure != crate::diagnostics::LaunchFailure::NativeAppExited)
        || record
            .child_exit
            .is_some_and(|exit| cfg!(windows) && matches!(exit, LaunchExit::Signal(_)))
    {
        return None;
    }
    let has_startup = record.startup_hint.is_some()
        || record.app_exit_code.is_some()
        || record.app_exit_signal.is_some();
    if has_startup
        && (spec.kind != DesktopHarnessKind::ChatGpt
            || record.startup_hint.is_none()
            || !matches!(
                record.failure,
                crate::diagnostics::LaunchFailure::NativeAppExited
                    | crate::diagnostics::LaunchFailure::NativeAlreadyRunning
            ))
    {
        return None;
    }
    if record.child_exit.is_some()
        && (record.app_exit_code.is_some()
            || record.app_exit_signal.is_some()
            || record.startup_hint.is_some())
    {
        return None;
    }
    Some(record)
}

async fn finish_scenario(
    spec: &ProbeSpec,
    process: &mut ProbeProcess,
    gui: Option<&Gui>,
    process_group: Option<ProcessGroupId>,
    outcome: Result<(), Reason>,
    gate: &ProviderGate,
    diagnostic: &mut Option<CleanupDiagnostic>,
) -> Result<(), Reason> {
    #[cfg(windows)]
    {
        process.cleanup_executable = Some(spec.executable.clone());
    }
    if let Err(failure) = stop(process, gui, process_group).await {
        *diagnostic = Some(CleanupDiagnostic {
            stage: CleanupStage::Stop,
            original_reason: outcome.as_ref().err().copied(),
            reason: Reason::CleanupFailed,
            absence: None,
            stop: Some(failure.diagnostic),
            restore: None,
        });
        return Err(Reason::CleanupFailed);
    }
    claude_storage::record(spec);
    claude_native_storage::record(spec);
    record_absence(
        Gui::ensure_absent_after_stop(
            spec.kind,
            gui,
            #[cfg(windows)]
            process.correlation_snapshot.take(),
            #[cfg(windows)]
            process.cleanup_holder.take(),
        ),
        CleanupStage::AbsenceAfterStop,
        outcome.err(),
        diagnostic,
    )?;
    record_restore(restore_detailed(spec).await, outcome.err(), diagnostic)?;
    record_absence(
        Gui::ensure_absent(spec.kind),
        CleanupStage::AbsenceAfterRestore,
        outcome.err(),
        diagnostic,
    )?;
    if gate.unauthorized() {
        return Err(Reason::InvalidKey);
    }
    if gate.budget_exceeded() {
        return Err(Reason::BudgetExceeded);
    }
    outcome
}

fn launcher_exit(status: std::process::ExitStatus) -> LaunchExit {
    if let Some(code) = status.code() {
        return LaunchExit::Code(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return LaunchExit::Signal(signal);
        }
    }
    LaunchExit::Unknown
}

fn record_absence(
    outcome: Result<(), crate::gui::AbsenceFailure>,
    stage: CleanupStage,
    original_reason: Option<Reason>,
    diagnostic: &mut Option<CleanupDiagnostic>,
) -> Result<(), Reason> {
    outcome.map_err(|failure| {
        *diagnostic = Some(CleanupDiagnostic {
            stage,
            original_reason,
            reason: failure.reason,
            absence: Some(failure.stage),
            stop: None,
            restore: None,
        });
        Reason::CleanupFailed
    })
}

fn live(
    gui: &Gui,
    _spec: &ProbeSpec,
    gate: &ProviderGate,
    fixture: &Path,
    marker: &str,
    result: &mut ProbeResult,
    composer_observations: &mut Vec<ComposerFailure>,
) -> Result<(), Reason> {
    let prompt = format!(
        "Use your file-reading tool to read {}. In your final response write NAN_CHECK_FINAL: immediately followed by the exact file contents. Do not guess or answer before the tool succeeds.",
        fixture.display()
    );
    let mode = submit(gui, &prompt, result, composer_observations)?;
    result.record_input(mode);
    result.steps.push(CheckStep::InputSubmitted);
    result.record_response(gui.wait_text(
        &format!("NAN_CHECK_FINAL:{marker}"),
        Duration::from_mins(2),
        composer_observations,
    )?);
    if !gate.response_verified() {
        return Err(Reason::ResponseMismatch);
    }
    result.steps.push(CheckStep::ResponseVerified);
    if !gate.tool_verified() {
        return Err(Reason::ToolMismatch);
    }
    result.steps.push(CheckStep::ToolVerified);
    Ok(())
}

async fn deterministic(
    gui: &Gui,
    inventory: &ScriptedProvider,
    gate: &ProviderGate,
    fixture: &Path,
    marker: &str,
    result: &mut ProbeResult,
    composer_observations: &mut Vec<ComposerFailure>,
) -> Result<(), Reason> {
    let mode = submit(gui, "Check this connection", result, composer_observations)?;
    result.record_input(mode);
    result.steps.push(CheckStep::InputSubmitted);
    let response = gui.wait_text(marker, Duration::from_secs(30), composer_observations);
    if response == Err(Reason::ResponseMismatch) && inventory.chat_requests().is_empty() {
        return Err(Reason::ProviderFailed);
    }
    result.record_response(response?);
    result.steps.push(CheckStep::ResponseVerified);
    let (name, input) =
        select_read_tool(&inventory.chat_requests(), fixture).ok_or(Reason::ToolMismatch)?;
    let tool_marker = visual_marker("NAN CHECK TOOL")?;
    let tool = ScriptedProvider::start(ProviderScenario::tool(name, input, &tool_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    gate.use_upstream(tool.base_url());
    // The private workspace is already open. Keep its temporary absolute path
    // in the tool contract, not in a narrow editable control verified by OCR.
    let mode = submit(
        gui,
        "Read read-target.txt using your file tool.",
        result,
        composer_observations,
    )?;
    result.record_input(mode);
    result.record_response(gui.wait_text(
        &tool_marker,
        Duration::from_secs(30),
        composer_observations,
    )?);
    if !tool.completed() || !tool.recording_bounded() || !gate.tool_verified() {
        return Err(Reason::ToolMismatch);
    }
    result.steps.push(CheckStep::ToolVerified);
    gate.fail_next_scenario(true);
    let mode = submit(
        gui,
        "Check the expected provider failure",
        result,
        composer_observations,
    )?;
    result.record_input(mode);
    result.record_response(gui.wait_text(
        "NAN_CHECK_EXPECTED_FAILURE",
        Duration::from_secs(20),
        composer_observations,
    )?);
    if !gate.failure_observed() {
        return Err(Reason::ProviderFailed);
    }
    gate.fail_next_scenario(false);
    let recovery_marker = visual_marker("NAN CHECK RECOVERED")?;
    let recovered = ScriptedProvider::start(ProviderScenario::inventory(&recovery_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    gate.use_upstream(recovered.base_url());
    let mode = submit(
        gui,
        "Try the connection again",
        result,
        composer_observations,
    )?;
    result.record_input(mode);
    result.record_response(gui.wait_text(
        &recovery_marker,
        Duration::from_secs(30),
        composer_observations,
    )?);
    result.steps.push(CheckStep::ErrorRecovered);
    Ok(())
}

fn submit(
    gui: &Gui,
    prompt: &str,
    result: &mut ProbeResult,
    composer_observations: &mut Vec<ComposerFailure>,
) -> Result<InputMode, Reason> {
    gui.submit(prompt)
        .inspect_err(|failure: &GuiFailure| {
            result.gui_stage = Some(failure.stage);
            if let Some(composer) = failure.composer {
                composer_observations.push(composer);
            }
        })
        .map_err(|failure| failure.reason)
}

fn select_read_tool(requests: &[Value], fixture: &Path) -> Option<(String, Value)> {
    for request in requests {
        let Some(tools) = request.get("tools").and_then(Value::as_array) else {
            continue;
        };
        for tool in tools {
            let Some(name) = tool.pointer("/function/name").and_then(Value::as_str) else {
                continue;
            };
            let input = match name {
                "Read" => json!({"file_path":fixture}),
                "read_file" => json!({"path":fixture}),
                "read_files" => json!({"paths":[fixture]}),
                "exec_command" => {
                    json!({"cmd":format!("cat -- '{}'", fixture.to_string_lossy().replace('\'', "'\\''"))})
                }
                _ => continue,
            };
            return Some((name.into(), input));
        }
    }
    None
}

/// Verify the diagnostic binding before any process runs.
fn prepare_launch_wrapper(kind: DesktopHarnessKind, wrapper: &LaunchWrapper) -> Result<(), Reason> {
    if kind != DesktopHarnessKind::ChatGpt || binary_digest(&wrapper.path)? != wrapper.sha256 {
        return Err(Reason::InstallationUnreadable);
    }
    // Never reuse a directory: each probe's facts must describe only its launch.
    nan_harness_private_fs::create_private_dir(&wrapper.facts)
        .map_err(|_| Reason::IsolationUnavailable)
}

fn prepare_claude_trial_roots(profile: &Path) -> Result<(), Reason> {
    // The ordinary CLI writes external config parents with create_dir_all.
    // This disposable trial needs private native roots before those writes,
    // without changing permissions or defaults for ordinary user profiles.
    let support = profile.join("home/Library/Application Support");
    for name in ["Claude", "Claude-3p"] {
        create_private_dir_all(&support.join(name)).map_err(|_| Reason::IsolationUnavailable)?;
    }
    Ok(())
}

fn isolated_command(spec: &ProbeSpec, program: &Path) -> Result<Command, Reason> {
    let profile = spec.workspace.join("profile");
    // Match the native Windows profile layout and verify known-folder lookup
    // on Windows; LOCALAPPDATA alone did not resolve for a fresh profile.
    let (local, roaming) = if cfg!(windows) {
        let app_data = profile.join("home").join("AppData");
        (app_data.join("Local"), app_data.join("Roaming"))
    } else {
        (profile.join("local"), profile.join("roaming"))
    };
    for directory in [
        &profile,
        &profile.join("home"),
        &profile.join("config"),
        &local,
        &roaming,
    ] {
        create_private_dir_all(directory).map_err(|_| Reason::IsolationUnavailable)?;
    }
    let native_policy = claude_native_roots::enabled(spec)?;
    if std::env::var_os("NANH_CLAUDE_MAC_PROFILE_POLICY").is_some() && !native_policy {
        if !cfg!(target_os = "macos")
            || spec.kind != DesktopHarnessKind::Claude
            || spec.session != crate::cli::SessionMode::GithubHosted
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
            || std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref()
                != Ok("electron-user-data-dir")
        {
            return Err(Reason::IsolationUnavailable);
        }
        prepare_claude_trial_roots(&profile)?;
    }
    let mut command = Command::new(program);
    command
        .arg(spec.kind.to_string())
        // Upstream apps discover global skills and credentials outside their
        // config directory. Redirect their home too, never the parent process.
        .env("CODEX_HOME", profile.join("home").join(".codex"))
        .env("NAN_HARNESS_CONFIG_DIR", profile.join("nanh"))
        // The probe owns its provider budget. A detached per-profile coordinator
        // would outlive the app and keep Windows recovery files locked.
        .env("NAN_HARNESS_INTERNAL_DISABLE_COORDINATOR", "1")
        .env("APPDATA", &roaming)
        .env("LOCALAPPDATA", &local)
        .env("HERMES_HOME", profile.join("hermes"))
        .env_remove("NAN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GITHUB_ENV")
        .env_remove("GITHUB_OUTPUT")
        .env_remove("GITHUB_PATH")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .env_remove("ACTIONS_RUNTIME_TOKEN")
        .current_dir(&spec.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if native_policy {
        command
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_STATE_HOME");
    } else {
        command
            .env("HOME", profile.join("home"))
            .env("USERPROFILE", profile.join("home"))
            .env("XDG_CONFIG_HOME", profile.join("config"));
    }
    // Keep the launched desktop app and any helper descendants in a probe-owned
    // group. Cleanup must be able to prove and remove the whole tree without
    // signalling an unrelated application in the runner's process group.
    #[cfg(unix)]
    command.process_group(0);
    // Claude only accepts its signed environment override. This trial uses the
    // native Electron argument, never inherited vendor authorization tokens.
    command
        .env_remove("CLAUDE_USER_DATA_DIR")
        .env_remove("CLAUDE_CDP_AUTH");
    if spec.kind == DesktopHarnessKind::ChatGpt {
        let user_data = profile.join("codex-desktop");
        let state = profile.join("nanh");
        let surface = state.join("chatgpt-desktop");
        let managed = surface.join("profile");
        // Prepare every owned ancestor before the CLI creates state files.
        // Its ordinary create_dir_all can otherwise leave intermediate Unix
        // directories with the runner's default permissions.
        for directory in [&user_data, &state, &surface, &managed] {
            create_private_dir_all(directory).map_err(|_| Reason::IsolationUnavailable)?;
        }
        // Codex's supported Electron override also isolates UI onboarding and
        // singleton state; CODEX_HOME alone redirects only runtime settings.
        command.env("CODEX_ELECTRON_USER_DATA_PATH", user_data);
    }
    if spec.kind == DesktopHarnessKind::Hermes {
        let user_data = hermes_user_data(&profile, &roaming);
        create_private_dir_all(&user_data).map_err(|_| Reason::IsolationUnavailable)?;
        // Electron's native userData lookup can ignore redirected HOME. Bind it
        // to the directory where nANH applies the managed active-profile file.
        command.env("HERMES_DESKTOP_USER_DATA_DIR", user_data);
    }
    if spec.kind == DesktopHarnessKind::Zed {
        command
            .arg("--user-data-dir")
            .arg(profile.join("zed"))
            .env("ZED_EXPERIMENTAL_A11Y", "1");
        if cfg!(target_os = "linux") {
            // Zed binds its single-instance socket inside the canonical data
            // directory, beyond sockaddr_un's limit in our journal hierarchy.
            // Stateless mode keeps probe databases in memory and omits that
            // socket; process/window ownership guards still exclude other apps.
            command.env("ZED_STATELESS", "1");
        }
    }
    Ok(command)
}

fn hermes_user_data(profile: &Path, roaming: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        profile.join("home/Library/Application Support/Hermes")
    } else if cfg!(windows) {
        roaming.join("Hermes")
    } else {
        profile.join("config/Hermes")
    }
}

fn prepare_zed_profile(spec: &ProbeSpec) -> Result<(), Reason> {
    if spec.kind != DesktopHarnessKind::Zed {
        return Ok(());
    }
    let directory = spec.workspace.join("profile").join("zed").join("config");
    create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
    // Never let a probe update an existing app or send diagnostics elsewhere.
    // Semantic probes fix the rem base used by the exact-source icon references.
    let settings: &[u8] = if spec.verification == crate::cli::VerificationPolicy::SemanticOnly {
        br#"{"auto_update":false,"telemetry":{"metrics":false,"diagnostics":false},"ui_font_size":16,"agent_ui_font_size":16,"agent_buffer_font_size":16}"#
    } else {
        br#"{"auto_update":false,"telemetry":{"metrics":false,"diagnostics":false},"agent_ui_font_size":18,"agent_buffer_font_size":16}"#
    };
    open_private_new(&directory.join("settings.json"))
        .and_then(|mut file| file.write_all(settings))
        .map_err(|_| Reason::IsolationUnavailable)?;
    if std::env::var_os("FEASIBILITY_ZED_NATIVE_COPY_FACTS").is_some()
        || spec.verification == crate::cli::VerificationPolicy::SemanticOnly
    {
        // HostedExperiment admits this opt-in only for an owned deterministic VM.
        // NewThread's workspace handler focuses the panel without toggling it.
        open_private_new(&directory.join("keymap.json"))
            .and_then(|mut file| {
                file.write_all(br#"[{"bindings":{"ctrl-alt-n":"agent::NewThread","ctrl-alt-y":"agent::CopyThreadToClipboard","ctrl-alt-z":"workspace::ToggleZoom"}},{"context":"SecurityModal","bindings":{"ctrl-alt-t":"menu::Confirm"}}]"#)
            })
            .map_err(|_| Reason::IsolationUnavailable)?;
    }
    Ok(())
}

fn endpoint_help_command(spec: &ProbeSpec) -> Command {
    let mut command = Command::new(&spec.nan_harness);
    command
        .arg(spec.kind.to_string())
        .arg("--help")
        .env_remove("NAN_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn require_endpoint_override(spec: &ProbeSpec) -> Result<(), Reason> {
    let mut child = endpoint_help_command(spec)
        .spawn()
        .map_err(|_| Reason::HarnessCapabilityUnavailable)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(Reason::HarnessCapabilityUnavailable)?;
    let operation = async {
        let mut bytes = Vec::new();
        stdout
            .take(65537)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Reason::HarnessCapabilityUnavailable)?;
        let status = child
            .wait()
            .await
            .map_err(|_| Reason::HarnessCapabilityUnavailable)?;
        if !status.success() || bytes.len() > 65536 {
            return Err(Reason::HarnessCapabilityUnavailable);
        }
        let help = String::from_utf8_lossy(&bytes);
        if !help
            .split_whitespace()
            .any(|word| word == "--provider-base-url")
        {
            return Err(Reason::HarnessCapabilityUnavailable);
        }
        if spec.kind == DesktopHarnessKind::Zed
            && !help
                .split_whitespace()
                .any(|word| word == "--user-data-dir")
        {
            return Err(Reason::HarnessCapabilityUnavailable);
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| Reason::HarnessCapabilityUnavailable)?
}

fn launch_command(spec: &ProbeSpec, gate: &ProviderGate) -> Result<Command, Reason> {
    let program = spec
        .launch_wrapper
        .as_ref()
        .map_or(spec.nan_harness.as_path(), |wrapper| wrapper.path.as_path());
    let mut command = isolated_command(spec, program)?;
    command.env_remove("NAN_NATIVE_OWNED_APP_LAUNCH");
    command.env_remove(PROCESS_OBSERVATION_ENV_PATH);
    if cfg!(target_os = "macos") && spec.kind == DesktopHarnessKind::Pen {
        command.env("NAN_NATIVE_OWNED_APP_LAUNCH", "1");
    }
    command.args([
        "--provider-base-url",
        &gate.base_url,
        "--model",
        &spec.model,
    ]);
    if spec.kind == DesktopHarnessKind::ChatGpt
        && spec.session == crate::cli::SessionMode::GithubHosted
    {
        // Cold-start observation can exceed the CLI's noninteractive default.
        // Keep one explicit deadline through any supervised owned relaunch.
        command.args(["--startup-timeout", "120"]);
    }
    if spec.kind == DesktopHarnessKind::Hermes {
        command.arg("--desktop-executable");
    } else {
        command.arg("--executable");
    }
    command
        .arg(&spec.executable)
        .env("NAN_API_KEY", gate.session_token())
        .env(
            "NAN_NATIVE_LAUNCH_DIAGNOSTIC",
            spec.workspace.join("native-launch-diagnostic.json"),
        );
    if cfg!(target_os = "macos") && spec.kind == DesktopHarnessKind::Claude {
        command.env(
            PROCESS_OBSERVATION_ENV_PATH,
            spec.workspace.join("native-process-observation.json"),
        );
    }
    if spec.kind == DesktopHarnessKind::Zed {
        command.arg(&spec.workspace);
    }
    if let Some(wrapper) = &spec.launch_wrapper {
        // The wrapper refuses unless the real binary still matches this digest.
        // Its reducer and bounds come from its own source directory and fixed
        // defaults, never from ambient overrides inherited by the probe.
        command
            .env("WAVE12_REAL_NANH", &spec.nan_harness)
            .env("WAVE12_REAL_SHA256", &spec.nan_harness_sha256)
            .env("WAVE12_FACTS_DIR", &wrapper.facts)
            .env(
                "WAVE12_DEADLINE_S",
                LAUNCH_WRAPPER_DEADLINE_SECONDS.to_string(),
            )
            .env_remove("WAVE12_REDUCER")
            .env_remove("WAVE12_PYTHON")
            .env_remove("WAVE12_GRACE_S")
            .env_remove("WAVE12_MAX_BYTES")
            .env_remove("WAVE12_MAX_LINE_BYTES")
            .env_remove("WAVE12_MAX_LINES");
    }
    Ok(command)
}

fn launch(
    spec: &ProbeSpec,
    gate: &ProviderGate,
) -> Result<ProbeProcess, (Reason, crate::diagnostics::LaunchFailure)> {
    let command = launch_command(spec, gate)
        .map_err(|reason| (reason, crate::diagnostics::LaunchFailure::LaunchSetup))?;
    claude_storage::capture(spec, &command);
    claude_native_storage::capture(spec);
    #[cfg(windows)]
    let cleanup_sha256 = (spec.kind == DesktopHarnessKind::Claude
        && spec.session == crate::cli::SessionMode::GithubHosted
        && std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() == Ok("private-env")
        && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline"))
    .then(|| binary_digest(&spec.executable).ok())
    .flatten();
    let process = ProbeProcess::spawn(command).map_err(|_| {
        (
            Reason::UnsupportedVersion,
            crate::diagnostics::LaunchFailure::LauncherSpawn,
        )
    })?;
    #[cfg(windows)]
    let process = {
        let mut process = process;
        process.cleanup_executable_sha256 = cleanup_sha256;
        process
    };
    Ok(process)
}

fn restore_command(spec: &ProbeSpec) -> Result<Command, Reason> {
    let mut command = isolated_command(spec, &spec.nan_harness)?;
    command.arg("--restore");
    Ok(command)
}

async fn restore(spec: &ProbeSpec) -> Result<(), Reason> {
    restore_detailed(spec)
        .await
        .map_err(|_| Reason::CleanupFailed)
}

async fn restore_detailed(spec: &ProbeSpec) -> Result<(), RestoreFailure> {
    let command = restore_command(spec).map_err(|_| RestoreFailure::CommandCreation)?;
    run_restore(command, Duration::from_secs(30)).await
}

async fn run_restore(mut command: Command, limit: Duration) -> Result<(), RestoreFailure> {
    command.kill_on_drop(true);
    let status = tokio::time::timeout(limit, command.status())
        .await
        .map_err(|_| RestoreFailure::DeadlineExpired)?
        .map_err(|_| RestoreFailure::ProcessIo)?;
    if status.success() {
        Ok(())
    } else {
        Err(RestoreFailure::NonzeroExit)
    }
}

fn record_restore(
    outcome: Result<(), RestoreFailure>,
    original_reason: Option<Reason>,
    diagnostic: &mut Option<CleanupDiagnostic>,
) -> Result<(), Reason> {
    outcome.map_err(|failure| {
        *diagnostic = Some(CleanupDiagnostic {
            stage: CleanupStage::Restore,
            original_reason,
            reason: Reason::CleanupFailed,
            absence: None,
            stop: None,
            restore: Some(failure),
        });
        Reason::CleanupFailed
    })
}

#[cfg(unix)]
type ProcessGroupId = i32;
#[cfg(windows)]
type ProcessGroupId = u32;

const TERM_GRACE: Duration = Duration::from_secs(2);

async fn wait_for_stop(process: &mut ProbeProcess, limit: Duration) -> StopWaitDiagnostic {
    let (outcome, os_error) = match tokio::time::timeout(limit, process.wait_launcher()).await {
        Ok(Ok(_)) => (StopWaitOutcome::Reaped, None),
        Ok(Err(error)) => (StopWaitOutcome::Failed, error.raw_os_error()),
        Err(_) => (StopWaitOutcome::TimedOut, None),
    };
    StopWaitDiagnostic { outcome, os_error }
}

#[cfg(windows)]
fn capture_stop_correlation(process: &mut ProbeProcess, gui: Option<&Gui>) -> Duration {
    let deadline = Instant::now() + Duration::from_secs(10);
    if let (Some(gui), Some(launcher)) = (gui, process.id()) {
        let query_deadline = deadline.min(Instant::now() + Duration::from_secs(1));
        process.correlation_snapshot = gui.capture_process_correlation(launcher, query_deadline);
        process.cleanup_holder = process
            .cleanup_executable
            .as_ref()
            .zip(process.cleanup_executable_sha256.as_deref())
            .and_then(|(expected, digest)| {
                gui.capture_owned_cleanup(launcher, expected, digest, query_deadline)
            });
    }
    // The advisory query consumes the existing initial stop budget.
    deadline.saturating_duration_since(Instant::now())
}

async fn stop(
    process: &mut ProbeProcess,
    gui: Option<&Gui>,
    process_group: Option<ProcessGroupId>,
) -> Result<(), StopFailure> {
    #[cfg(windows)]
    debug_assert!(process_group.is_none());
    #[cfg(windows)]
    let initial_wait_limit = capture_stop_correlation(process, gui);
    #[cfg(not(windows))]
    let initial_wait_limit = Duration::from_secs(10);
    if let Some(gui) = gui {
        let _ = gui.quit();
    }
    let mut diagnostic = StopDiagnostic {
        initial_wait: StopWaitDiagnostic {
            outcome: StopWaitOutcome::TimedOut,
            os_error: None,
        },
        grace_wait: StopWaitDiagnostic {
            outcome: StopWaitOutcome::NotAttempted,
            os_error: None,
        },
        kill: StopKillDiagnostic {
            outcome: StopKillOutcome::NotAttempted,
            os_error: None,
        },
        final_wait: StopWaitDiagnostic {
            outcome: StopWaitOutcome::NotAttempted,
            os_error: None,
        },
    };
    diagnostic.initial_wait = wait_for_stop(process, initial_wait_limit).await;
    if diagnostic.initial_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(unix)]
        if process_group.is_none_or(group_absent) {
            return Ok(());
        }
        #[cfg(windows)]
        return close_owned_job(process, diagnostic);
    }
    #[cfg(unix)]
    if let Some(group) =
        process_group.or_else(|| process.id().and_then(|pid| i32::try_from(pid).ok()))
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(-group),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    // Windows cleanup remains handle-based. A retained numeric PID is not
    // authority to kill a tree after wait() has reaped the launcher.
    diagnostic.grace_wait = wait_for_stop(process, TERM_GRACE).await;
    if diagnostic.grace_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(unix)]
        if process_group.is_none_or(group_absent) {
            return Ok(());
        }
        #[cfg(windows)]
        return close_owned_job(process, diagnostic);
    }
    #[cfg(unix)]
    if let Some(group) =
        process_group.or_else(|| process.id().and_then(|pid| i32::try_from(pid).ok()))
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(-group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    match tokio::time::timeout(TERM_GRACE, process.kill()).await {
        Ok(Ok(())) => diagnostic.kill.outcome = StopKillOutcome::Issued,
        Ok(Err(error)) => {
            diagnostic.kill.outcome = StopKillOutcome::Failed;
            diagnostic.kill.os_error = error.raw_os_error();
        }
        Err(_) => diagnostic.kill.outcome = StopKillOutcome::TimedOut,
    }
    diagnostic.final_wait = wait_for_stop(process, TERM_GRACE).await;
    if diagnostic.final_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(windows)]
        {
            process.close_job();
            return Ok(());
        }
    }
    #[cfg(unix)]
    if let Some(group) = process_group {
        // A killed descendant may remain a zombie until its reaper observes
        // it. Poll the dedicated group for bounded absence proof rather than
        // treating the launcher's exit as proof that the tree is gone.
        for _ in 0..50 {
            if group_absent(group) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    } else {
        return Ok(());
    }
    Err(StopFailure { diagnostic })
}

#[cfg(windows)]
fn close_owned_job(
    process: &mut ProbeProcess,
    mut diagnostic: StopDiagnostic,
) -> Result<(), StopFailure> {
    // Launcher exit does not prove descendant exit. Terminate the owned job and close
    // its kill-on-close handle before the separate bounded native absence checks.
    let result = process.start_kill();
    process.close_job();
    result.map_err(|error| {
        diagnostic.kill.outcome = StopKillOutcome::Failed;
        diagnostic.kill.os_error = error.raw_os_error();
        StopFailure { diagnostic }
    })
}

#[cfg(unix)]
fn group_absent(group: ProcessGroupId) -> bool {
    // Signal zero observes existence without resuming a stopped process.
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(-group), None)
        .is_err_and(|error| error == nix::errno::Errno::ESRCH)
}

fn visual_marker(label: &str) -> Result<String, Reason> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Reason::ProviderFailed)?;
    Ok(encode_visual_marker(label, &bytes))
}

fn encode_visual_marker(label: &str, bytes: &[u8; 16]) -> String {
    // Each nibble has a distinct ordinary word. Keep all 128 random bits and
    // exact matching. Uppercase avoids OCR inventing sentence-case transitions
    // at wrapped line starts in the synthetic transcript.
    const WORDS: [&str; 16] = [
        "APPLE", "BREAD", "CHAIR", "DREAM", "EAGLE", "FIELD", "GREEN", "HOUSE", "ISLAND", "JUICE",
        "KITE", "LEMON", "MOON", "NORTH", "OCEAN", "PAPER",
    ];
    std::iter::once(label)
        .chain(
            bytes
                .iter()
                .flat_map(|byte| [WORDS[usize::from(byte >> 4)], WORDS[usize::from(byte & 15)]]),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn claude_trial_precreates_private_roots_before_external_config_writes() {
        use std::os::unix::fs::PermissionsExt as _;
        let workspace = tempfile::tempdir().unwrap();
        let profile = workspace.path().join("profile");
        prepare_claude_trial_roots(&profile).unwrap();
        for name in ["Claude", "Claude-3p"] {
            let root = profile.join("home/Library/Application Support").join(name);
            // Match external atomic_write's parent creation: it preserves an
            // existing directory, rather than making this trial root private.
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("claude_desktop_config.json"), b"{}").unwrap();
            assert!(
                root.metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    .trailing_zeros()
                    >= 6
            );
        }
    }

    use super::*;

    fn assert_executable_argument(kind: DesktopHarnessKind, spec: &ProbeSpec, args: &[String]) {
        let executable_flag = if kind == DesktopHarnessKind::Hermes {
            "--desktop-executable"
        } else {
            "--executable"
        };
        assert!(
            args.windows(2).any(|pair| {
                pair == [executable_flag, spec.executable.to_string_lossy().as_ref()]
            })
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_stop_reaps_a_synthetic_child_after_handle_kill() {
        use tokio::io::AsyncReadExt as _;
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::Out.Write('ready'); [Console]::Out.Flush(); Start-Sleep -Seconds 60",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut process = ProbeProcess::spawn(command).unwrap();
        let mut stdout = process.take_stdout().unwrap();
        let pid = process.id().unwrap();
        let mut ready = [0; 5];
        tokio::time::timeout(Duration::from_secs(10), stdout.read_exact(&mut ready))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&ready, b"ready");
        assert_eq!(stop(&mut process, None, None).await, Ok(()));
        assert!(process.id().is_none());
        assert!(!windows_process_tests::descendant_alive(pid).await);
    }

    #[test]
    fn tool_discovery_skips_non_function_tools_and_unrelated_requests() {
        let fixture = Path::new("/synthetic/read-target.txt");
        let requests = vec![
            json!({"tools": [{"type": "web_search"}]}),
            json!({"tools": [{"function": {}}, {"function": {"name": "read_file"}}]}),
        ];
        assert_eq!(
            select_read_tool(&requests, fixture),
            Some(("read_file".into(), json!({"path": fixture})))
        );
        assert!(select_read_tool(&requests[..1], fixture).is_none());
    }

    #[test]
    fn launch_exit_accepts_only_closed_numeric_evidence() {
        let value = serde_json::to_value(LaunchExit::Code(7)).unwrap();
        assert_eq!(value, json!({"code": 7}));
        assert_eq!(
            serde_json::from_value::<LaunchExit>(value).unwrap(),
            LaunchExit::Code(7)
        );
        for invalid in [
            json!("private app output"),
            json!({"code": "private app output"}),
            json!({"code": 7, "output": "private app output"}),
        ] {
            assert!(serde_json::from_value::<LaunchExit>(invalid).is_err());
        }
        assert!(valid_discovery_exit(LaunchExit::Signal(1)));
        assert!(valid_discovery_exit(LaunchExit::Signal(127)));
        for signal in [0, -1, 128] {
            assert!(!valid_discovery_exit(LaunchExit::Signal(signal)));
        }
    }

    #[test]
    fn native_process_observation_rejects_malformed_and_oversized_records() {
        let present = decode_native_process_observation(
            br#"{"schemaVersion":1,"observation":"matching-process-present","everObservedPresent":true}"#,
        )
        .unwrap();
        assert_eq!(
            present.state,
            crate::diagnostics::NativeProcessObservationState::MatchingProcessPresent
        );
        let absent = decode_native_process_observation(
            br#"{"schemaVersion":1,"observation":"matching-process-absent","everObservedPresent":true}"#,
        )
        .unwrap();
        assert_eq!(
            absent.state,
            crate::diagnostics::NativeProcessObservationState::MatchingProcessAbsent
        );
        let failed = decode_native_process_observation(
            br#"{"schemaVersion":1,"observation":"query-failed","everObservedPresent":true}"#,
        )
        .unwrap();
        assert_eq!(
            failed.state,
            crate::diagnostics::NativeProcessObservationState::QueryFailed
        );
        assert!(decode_native_process_observation(br#"{"schemaVersion":2}"#).is_none());
        assert!(decode_native_process_observation(&vec![b'x'; 513]).is_none());
        assert!(decode_native_process_observation(
            br#"{"schemaVersion":1,"observation":"matching-process-present","everObservedPresent":true,"path":"private"}"#
        )
        .is_none());
        assert!(decode_native_process_observation(
            br#"{"schemaVersion":1,"observation":"matching-process-present","everObservedPresent":false}"#
        )
        .is_none());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn claude_identity_observation_is_disabled_off_macos() {
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::Claude,
            nan_harness: "/missing/nanh".into(),
            nan_harness_sha256: "0".repeat(64),
            executable: "/missing/Claude.app/Contents/MacOS/Claude".into(),
            workspace: "/missing/workspace".into(),
            model: "model".into(),
            live: false,
            probe_index: Some(0),
            session: crate::cli::SessionMode::default(),
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        assert!(read_claude_identity_observation(&spec, true).is_none());
    }

    #[test]
    fn identity_capture_requires_failed_acquisition_and_claude_kind() {
        let mut spec = ProbeSpec {
            kind: DesktopHarnessKind::Claude,
            nan_harness: "/missing/nanh".into(),
            nan_harness_sha256: "0".repeat(64),
            executable: "/missing/Claude.app/Contents/MacOS/Claude".into(),
            workspace: "/missing/workspace".into(),
            model: "model".into(),
            live: false,
            probe_index: Some(0),
            session: crate::cli::SessionMode::default(),
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        assert!(!identity_capture_allowed(&spec, false));
        spec.kind = DesktopHarnessKind::Pen;
        assert!(!identity_capture_allowed(&spec, true));
    }

    fn assert_launch_failure_records(spec: &ProbeSpec, path: &Path) {
        use crate::diagnostics::LaunchFailure;
        for (failure, expected) in [
            ("provider-routing-failed", LaunchFailure::ProviderRouting),
            (
                "argument-validation-failed",
                LaunchFailure::ArgumentValidation,
            ),
            ("native-app-spawn-failed", LaunchFailure::NativeAppSpawn),
            (
                "native-capability-missing",
                LaunchFailure::NativeCapabilityMissing,
            ),
            (
                "native-version-unparseable",
                LaunchFailure::NativeVersionUnparseable,
            ),
        ] {
            std::fs::write(
                path,
                serde_json::to_vec(&json!({"schemaVersion": 1, "failure": failure})).unwrap(),
            )
            .unwrap();
            assert_eq!(
                read_child_launch_diagnostic(spec).map(|record| record.failure),
                Some(expected)
            );
        }
    }

    #[test]
    fn child_launch_failure_read_is_bounded_and_works_without_wrapper() {
        let directory = tempfile::tempdir().unwrap();
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::Hermes,
            nan_harness: PathBuf::from("/nanh"),
            nan_harness_sha256: "a".repeat(64),
            executable: PathBuf::from("/app"),
            workspace: directory.path().to_path_buf(),
            model: "model".into(),
            live: false,
            probe_index: Some(0),
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        let path = spec.workspace.join("native-launch-diagnostic.json");
        assert_launch_failure_records(&spec, &path);
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"launch-setup-failed","setupCause":"runtime"}"#,
        )
        .unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).and_then(|record| record.setup_cause),
            Some(crate::diagnostics::SetupCause::Runtime)
        );
        std::fs::write(
            &path,
            include_bytes!("../../../canary/tests/fixtures/native-discovery-cause.json"),
        )
        .unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).and_then(|record| record.discovery_cause),
            Some(crate::diagnostics::DiscoveryCause::VersionCommandFailed)
        );
        std::fs::write(
            &path,
            include_bytes!("../../../canary/tests/fixtures/native-setup-cause.json"),
        )
        .unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).and_then(|record| record.setup_cause),
            Some(crate::diagnostics::SetupCause::Runtime)
        );
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"native-app-spawn-failed","setupCause":"runtime"}"#,
        )
        .unwrap();
        assert!(read_child_launch_diagnostic(&spec).is_none());
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"launch-setup-failed","setupCause":"runtime","discoveryCause":"missing-executable"}"#,
        )
        .unwrap();
        assert!(read_child_launch_diagnostic(&spec).is_none());
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"launch-setup-failed","setupCause":"private"}"#,
        )
        .unwrap();
        assert!(read_child_launch_diagnostic(&spec).is_none());
        for value in [
            serde_json::json!({"schemaVersion":1,"failure":"native-app-exited","childExit":{"code":17}}),
            serde_json::json!({"schemaVersion":1,"failure":"native-app-exited","childExit":{"signal":9}}),
        ] {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            let record = read_child_launch_diagnostic(&spec).unwrap();
            assert_eq!(
                record.child_exit,
                Some(if value["childExit"].get("code").is_some() {
                    LaunchExit::Code(17)
                } else {
                    LaunchExit::Signal(9)
                })
            );
        }
        for value in [
            serde_json::json!({"schemaVersion":1,"failure":"native-app-exited","childExit":{"signal":0}}),
            serde_json::json!({"schemaVersion":1,"failure":"native-app-exited","childExit":{"code":0}}),
            serde_json::json!({"schemaVersion":1,"failure":"native-app-spawn-failed","childExit":{"code":17}}),
        ] {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(read_child_launch_diagnostic(&spec).is_none());
        }
        std::fs::write(&path, vec![b'x'; 1025]).unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).map(|record| record.failure),
            None
        );
        std::fs::write(&path, br#"{"schemaVersion":1,"failure":"private"}"#).unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).map(|record| record.failure),
            None
        );
        assert_chatgpt_startup_diagnostic(&spec, &path);
    }

    fn assert_chatgpt_startup_diagnostic(spec: &ProbeSpec, path: &Path) {
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::ChatGpt,
            ..spec.clone()
        };
        let valid = json!({"schemaVersion":1,"failure":"native-app-exited",
            "appExitSignal":6,"startupHint":"no-usable-sandbox"});
        std::fs::write(path, serde_json::to_vec(&valid).unwrap()).unwrap();
        assert_eq!(
            read_child_launch_diagnostic(&spec).unwrap().startup(),
            Some(crate::diagnostics::StartupDiagnostic {
                exit: Some(LaunchExit::Signal(6)),
                hint: crate::diagnostics::StartupHint::NoUsableSandbox,
                sandbox: None,
            })
        );
        for (key, value) in [
            ("appExitCode", json!(1)),
            ("appExitSignal", json!(0)),
            ("startupHint", json!("private")),
            ("stderr", json!("private")),
            ("failure", json!("native-capability-missing")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            std::fs::write(path, serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert!(read_child_launch_diagnostic(&spec).is_none());
        }
    }

    #[test]
    fn child_discovery_signal_validation_is_closed_and_context_bound() {
        let directory = tempfile::tempdir().unwrap();
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::ChatGpt,
            nan_harness: PathBuf::from("/nanh"),
            nan_harness_sha256: "0".repeat(64),
            executable: PathBuf::from("/app"),
            workspace: directory.path().to_path_buf(),
            model: "model".into(),
            live: false,
            probe_index: Some(0),
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        let path = spec.workspace.join("native-launch-diagnostic.json");
        for signal in [1, 127] {
            let value = format!(
                "{{\"schemaVersion\":1,\"failure\":\"launch-setup-failed\",\"setupCause\":\"discovery\",\"discoveryCause\":\"version-command-failed\",\"discoveryExit\":{{\"signal\":{signal}}}}}"
            );
            std::fs::write(&path, value).unwrap();
            assert!(read_child_launch_diagnostic(&spec).is_some());
        }
        for signal in [0, 128] {
            let value = format!(
                "{{\"schemaVersion\":1,\"failure\":\"launch-setup-failed\",\"setupCause\":\"discovery\",\"discoveryCause\":\"version-command-failed\",\"discoveryExit\":{{\"signal\":{signal}}}}}"
            );
            std::fs::write(&path, value).unwrap();
            assert!(read_child_launch_diagnostic(&spec).is_none());
        }
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"launch-setup-failed","setupCause":"runtime","discoveryCause":"version-command-failed","discoveryExit":{"signal":9}}"#,
        )
        .unwrap();
        assert!(read_child_launch_diagnostic(&spec).is_none());
        std::fs::write(
            &path,
            br#"{"schemaVersion":1,"failure":"launch-setup-failed","setupCause":"discovery","discoveryCause":"version-command-failed","discoveryExit":{"signal":9,"status":9}}"#,
        )
        .unwrap();
        assert!(read_child_launch_diagnostic(&spec).is_none());
    }

    #[test]
    fn composer_diagnostic_is_closed_and_operation_specific() {
        let diagnostic = ComposerDiagnostic {
            schema_version: 1,
            observations: vec![ComposerFailure {
                operation: crate::gui::ComposerOperation::TypeText,
                error_category: crate::gui::ComposerErrorCategory::ActionUnsupported,
                guard_context: None,
                geometry_relation: None,
                input_observation: None,
            }],
        };
        let value = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(
            value,
            json!({
                "schemaVersion": 1,
                "observations": [{
                    "operation": "type-text",
                    "errorCategory": "action-unsupported"
                }]
            })
        );
        assert!(serde_json::from_value::<ComposerDiagnostic>(value).is_ok());
        for invalid in [
            json!({"schemaVersion": 1, "observations": [], "raw": "selector"}),
            json!({"schemaVersion": 1, "observations": [{"operation": "raw", "errorCategory": "other"}]}),
            json!({"schemaVersion": 1, "observations": [{"operation": "type-text", "errorCategory": "raw"}]}),
        ] {
            assert!(serde_json::from_value::<ComposerDiagnostic>(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn launch_exit_distinguishes_process_codes_and_signals() {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(
            launcher_exit(std::process::ExitStatus::from_raw(7 << 8)),
            LaunchExit::Code(7)
        );
        let signal = nix::sys::signal::Signal::SIGTERM as i32;
        assert_eq!(
            launcher_exit(std::process::ExitStatus::from_raw(signal)),
            LaunchExit::Signal(signal)
        );
    }

    #[test]
    fn digest_streams_large_files_within_the_identity_contract() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large-executable");
        let mut file = std::fs::File::create(&path).unwrap();
        let mut bytes = Vec::new();
        // Cross chunk boundaries with a size that is not a chunk multiple.
        bytes.extend_from_slice(&vec![7u8; 128 * 1024 + 3]);
        bytes.extend_from_slice(&vec![9u8; 256 * 1024]);
        bytes.extend_from_slice(b"final identity bytes");
        file.write_all(&bytes).unwrap();
        assert_eq!(binary_digest(&path).unwrap(), crate::report::digest(&bytes));
    }

    #[test]
    fn digest_rejects_directories_and_oversized_files_without_buffering_them() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            binary_digest(directory.path()),
            Err(Reason::InstallationUnreadable)
        );
        let oversized = directory.path().join("oversized-executable");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_DIGESTED_BYTES + 1)
            .unwrap();
        assert_eq!(
            binary_digest(&oversized),
            Err(Reason::InstallationUnreadable)
        );
    }

    #[test]
    fn digest_accepts_a_file_at_the_exact_identity_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("limit-executable");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"header").unwrap();
        file.set_len(MAX_DIGESTED_BYTES).unwrap();
        assert!(binary_digest(&path).is_ok());
    }

    #[test]
    fn restoration_failure_preserves_original_outcome_without_command_details() {
        for failure in [
            RestoreFailure::CommandCreation,
            RestoreFailure::ProcessIo,
            RestoreFailure::DeadlineExpired,
            RestoreFailure::NonzeroExit,
        ] {
            let mut diagnostic = None;
            assert_eq!(
                record_restore(
                    Err(failure),
                    Some(Reason::ActionUnsupported),
                    &mut diagnostic
                ),
                Err(Reason::CleanupFailed)
            );
            let value = serde_json::to_value(diagnostic.unwrap()).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 4);
            assert_eq!(value["stage"], "restore");
            assert_eq!(value["reason"], "cleanup-failed");
            assert_eq!(value["originalReason"], "action-unsupported");
            assert_eq!(
                serde_json::from_value::<RestoreFailure>(value["restore"].clone()).unwrap(),
                failure
            );
        }
        let mut diagnostic = None;
        assert_eq!(record_restore(Ok(()), None, &mut diagnostic), Ok(()));
        assert!(diagnostic.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restoration_distinguishes_process_io_deadline_and_nonzero_exit() {
        let missing = Command::new("/nanh-synthetic-nonexistent-restore");
        assert_eq!(
            run_restore(missing, Duration::from_secs(1)).await,
            Err(RestoreFailure::ProcessIo)
        );
        let mut nonzero = Command::new("sh");
        nonzero.args(["-c", "exit 7"]);
        assert_eq!(
            run_restore(nonzero, Duration::from_secs(1)).await,
            Err(RestoreFailure::NonzeroExit)
        );
        let mut slow = Command::new("sleep");
        slow.arg("10");
        assert_eq!(
            run_restore(slow, Duration::from_millis(10)).await,
            Err(RestoreFailure::DeadlineExpired)
        );
    }

    #[test]
    fn worker_diagnostics_reject_unstructured_details() {
        let mut value = json!({ "stage": "restore", "originalReason": "selector-not-matched", "reason": "cleanup-failed" });
        assert!(serde_json::from_value::<CleanupDiagnostic>(value.clone()).is_ok());
        value["message"] = json!("synthetic native message");
        assert!(serde_json::from_value::<CleanupDiagnostic>(value).is_err());
    }

    #[test]
    fn stop_diagnostic_is_closed_and_preserves_each_synthetic_outcome() {
        let diagnostic = CleanupDiagnostic {
            stage: CleanupStage::Stop,
            original_reason: Some(Reason::SelectorNotMatched),
            reason: Reason::CleanupFailed,
            absence: None,
            restore: None,
            stop: Some(StopDiagnostic {
                initial_wait: StopWaitDiagnostic {
                    outcome: StopWaitOutcome::TimedOut,
                    os_error: None,
                },
                grace_wait: StopWaitDiagnostic {
                    outcome: StopWaitOutcome::Failed,
                    os_error: Some(-5),
                },
                kill: StopKillDiagnostic {
                    outcome: StopKillOutcome::Issued,
                    os_error: None,
                },
                final_wait: StopWaitDiagnostic {
                    outcome: StopWaitOutcome::TimedOut,
                    os_error: None,
                },
            }),
        };
        let value = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(value["stop"]["initialWait"]["outcome"], "timed-out");
        assert_eq!(value["stop"]["graceWait"]["outcome"], "failed");
        assert_eq!(value["stop"]["graceWait"]["osError"], -5);
        assert_eq!(value["stop"]["kill"]["outcome"], "issued");
        assert_eq!(value["stop"]["finalWait"]["outcome"], "timed-out");
        assert!(serde_json::from_value::<CleanupDiagnostic>(value.clone()).is_ok());
        let mut extra = value;
        extra["stop"]["graceWait"]["pid"] = json!(42);
        assert!(serde_json::from_value::<CleanupDiagnostic>(extra).is_err());
    }

    #[test]
    fn stop_diagnostic_rejects_os_error_values_outside_i32() {
        let value = json!({
            "stage": "stop",
            "originalReason": "selector-not-matched",
            "reason": "cleanup-failed",
            "stop": {
                "initialWait": {"outcome": "timed-out"},
                "graceWait": {"outcome": "timed-out"},
                "kill": {"outcome": "failed", "osError": 2_147_483_648_u64},
                "finalWait": {"outcome": "failed"}
            }
        });
        assert!(serde_json::from_value::<CleanupDiagnostic>(value).is_err());
    }

    #[test]
    fn absence_diagnostics_survive_the_private_worker_envelope() {
        for absence in [
            crate::gui::AbsenceStage::AccessibilityProvider,
            crate::gui::AbsenceStage::AccessibilityEnumeration,
            crate::gui::AbsenceStage::NativeWindows,
        ] {
            let mut cleanup = None;
            assert_eq!(
                record_absence(
                    Err(crate::gui::AbsenceFailure {
                        stage: absence,
                        reason: Reason::ActionUnsupported
                    }),
                    CleanupStage::AbsenceAfterStop,
                    Some(Reason::SelectorNotMatched),
                    &mut cleanup,
                ),
                Err(Reason::CleanupFailed)
            );
            let outcome = WorkerOutcome {
                result: ProbeResult::blocked(Reason::CleanupFailed),
                launch_exit: None,
                child_exit: None,
                discovery_exit: None,
                launch_failure: None,
                setup_cause: None,
                discovery_cause: None,
                startup: None,
                native_process_observation: None,
                claude_identity_observation: None,
                claude_readiness: None,
                matched_window_inventory: None,
                cleanup,
                composer: Vec::new(),
                gui_acquisition: None,
            };
            let decoded: WorkerOutcome =
                serde_json::from_slice(&serde_json::to_vec(&outcome).unwrap()).unwrap();
            assert_eq!(decoded.cleanup, outcome.cleanup);
            assert_eq!(decoded.cleanup.unwrap().absence, Some(absence));
        }
    }

    #[test]
    fn visual_markers_preserve_every_random_nibble_in_readable_words() {
        let baseline = encode_visual_marker("RESPONSE", &[0; 16]);
        assert_eq!(baseline.split_whitespace().count(), 33);
        let mut encodings = std::collections::BTreeSet::new();
        for offset in 0..16 {
            for value in 1..=u8::MAX {
                let mut bytes = [0; 16];
                bytes[offset] = value;
                let encoded = encode_visual_marker("RESPONSE", &bytes);
                assert_ne!(encoded, baseline);
                assert!(encodings.insert(encoded));
            }
        }
        assert!(encode_visual_marker("RESPONSE", &[255; 16]).ends_with("PAPER PAPER"));
    }

    #[tokio::test]
    async fn owned_app_launch_is_exclusive_to_pen_macos() {
        let directory = tempfile::tempdir().unwrap();
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("synthetic-provider-key".into()),
            false,
            "fixture-marker",
        )
        .await
        .unwrap();
        for kind in DesktopHarnessKind::ALL {
            let spec = ProbeSpec {
                kind,
                nan_harness: directory.path().join("nanh"),
                nan_harness_sha256: "a".repeat(64),
                executable: directory.path().join("app"),
                workspace: directory.path().join(kind.to_string()),
                model: "qwen3.6".into(),
                live: false,
                probe_index: None,
                session: crate::cli::SessionMode::PrivateProfile,
                verification: crate::cli::VerificationPolicy::default(),
                launch_wrapper: None,
            };
            let command = launch_command(&spec, &gate).unwrap();
            let owned_launch = command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == "NAN_NATIVE_OWNED_APP_LAUNCH")
                .unwrap()
                .1;
            let expected = (cfg!(target_os = "macos") && kind == DesktopHarnessKind::Pen)
                .then_some(std::ffi::OsStr::new("1"));
            assert_eq!(owned_launch, expected);
            let observation = command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == PROCESS_OBSERVATION_ENV_PATH)
                .map(|(_, value)| value);
            let expected_observation_path = spec.workspace.join("native-process-observation.json");
            let expected_observation = (cfg!(target_os = "macos")
                && kind == DesktopHarnessKind::Claude)
                .then_some(expected_observation_path.as_os_str());
            assert_eq!(observation, Some(expected_observation));
        }
    }

    #[test]
    fn codex_launch_isolates_electron_data_as_well_as_runtime_settings() {
        let directory = tempfile::tempdir().unwrap();
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::ChatGpt,
            nan_harness: directory.path().join("nanh"),
            nan_harness_sha256: "a".repeat(64),
            executable: directory.path().join("app"),
            workspace: directory.path().join("workspace"),
            model: "qwen3.6".into(),
            live: false,
            probe_index: None,
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        let command = isolated_command(&spec, &spec.nan_harness).unwrap();
        let data = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "CODEX_ELECTRON_USER_DATA_PATH")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(
            Path::new(data),
            spec.workspace.join("profile/codex-desktop")
        );
        assert!(Path::new(data).is_dir());
        for suffix in [
            "nanh",
            "nanh/chatgpt-desktop",
            "nanh/chatgpt-desktop/profile",
        ] {
            let path = spec.workspace.join("profile").join(suffix);
            assert!(path.is_dir());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
                    0
                );
            }
        }
    }

    #[test]
    fn hermes_launch_redirects_native_data_to_the_managed_profile_directory() {
        let directory = tempfile::tempdir().unwrap();
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::Hermes,
            nan_harness: directory.path().join("nanh"),
            nan_harness_sha256: "a".repeat(64),
            executable: directory.path().join("app"),
            workspace: directory.path().join("workspace"),
            model: "qwen3.6".into(),
            live: false,
            probe_index: None,
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        let command = isolated_command(&spec, &spec.nan_harness).unwrap();
        let data = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "HERMES_DESKTOP_USER_DATA_DIR")
            .unwrap()
            .1
            .unwrap();
        let expected = if cfg!(target_os = "macos") {
            "profile/home/Library/Application Support/Hermes"
        } else if cfg!(windows) {
            "profile/home/AppData/Roaming/Hermes"
        } else {
            "profile/config/Hermes"
        };
        assert_eq!(Path::new(data), spec.workspace.join(expected));
        assert!(Path::new(data).is_dir());
    }

    #[tokio::test]
    async fn only_hosted_codex_uses_an_explicit_cold_start_deadline() {
        use crate::cli::SessionMode;
        let directory = tempfile::tempdir().unwrap();
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("synthetic-provider-key".into()),
            false,
            "fixture-marker",
        )
        .await
        .unwrap();
        for (kind, session, expected) in [
            (DesktopHarnessKind::ChatGpt, SessionMode::GithubHosted, true),
            (
                DesktopHarnessKind::ChatGpt,
                SessionMode::PrivateProfile,
                false,
            ),
            (DesktopHarnessKind::Hermes, SessionMode::GithubHosted, false),
        ] {
            let spec = ProbeSpec {
                kind,
                nan_harness: directory.path().join("nanh"),
                nan_harness_sha256: "a".repeat(64),
                executable: directory.path().join("app"),
                workspace: directory.path().join(kind.to_string()),
                model: "qwen3.6".into(),
                live: false,
                probe_index: None,
                session,
                verification: crate::cli::VerificationPolicy::default(),
                launch_wrapper: None,
            };
            let command = launch_command(&spec, &gate).unwrap();
            let args = command.as_std().get_args().collect::<Vec<_>>();
            assert_eq!(
                args.windows(2).any(|pair| pair
                    == [
                        std::ffi::OsStr::new("--startup-timeout"),
                        std::ffi::OsStr::new("120")
                    ]),
                expected
            );
        }
    }

    #[tokio::test]
    async fn every_app_uses_the_local_endpoint_and_only_a_session_token() {
        let directory = tempfile::tempdir().unwrap();
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("synthetic-provider-key".into()),
            false,
            "fixture-marker",
        )
        .await
        .unwrap();
        for kind in DesktopHarnessKind::ALL {
            let spec = ProbeSpec {
                kind,
                nan_harness: directory.path().join("nanh"),
                nan_harness_sha256: "a".repeat(64),
                executable: directory.path().join("app"),
                workspace: directory.path().join(kind.to_string()),
                model: "qwen3.6".into(),
                live: false,
                probe_index: None,
                session: crate::cli::SessionMode::PrivateProfile,
                verification: crate::cli::VerificationPolicy::default(),
                launch_wrapper: None,
            };
            let command = launch_command(&spec, &gate).unwrap();
            let args = command
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                args.windows(2)
                    .any(|pair| pair == ["--provider-base-url", gate.base_url.as_str()])
            );
            assert_executable_argument(kind, &spec, &args);
            let key = command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == "NAN_API_KEY")
                .unwrap()
                .1
                .unwrap();
            assert_eq!(key, gate.session_token());
            assert_ne!(key, "synthetic-provider-key");
            assert!(command.as_std().get_envs().any(|(name, value)| {
                name == "NAN_HARNESS_INTERNAL_DISABLE_COORDINATOR"
                    && value == Some(std::ffi::OsStr::new("1"))
            }));
            for variable in ["HOME", "USERPROFILE"] {
                let home = command
                    .as_std()
                    .get_envs()
                    .find(|(name, _)| *name == variable)
                    .unwrap()
                    .1
                    .unwrap();
                assert_eq!(Path::new(home), spec.workspace.join("profile").join("home"));
            }
            if kind == DesktopHarnessKind::Zed {
                if cfg!(target_os = "linux") {
                    assert!(command.as_std().get_envs().any(|(key, value)| {
                        key == "ZED_STATELESS" && value == Some(std::ffi::OsStr::new("1"))
                    }));
                }
                let profile = spec.workspace.join("profile").join("zed");
                assert!(args.windows(2).any(|pair| {
                    pair[0] == "--user-data-dir" && pair[1] == profile.to_string_lossy()
                }));
            }
            if cfg!(windows) {
                for (variable, name) in [("LOCALAPPDATA", "Local"), ("APPDATA", "Roaming")] {
                    let path = command
                        .as_std()
                        .get_envs()
                        .find(|(key, _)| *key == variable)
                        .unwrap()
                        .1
                        .unwrap();
                    assert_eq!(
                        Path::new(path),
                        spec.workspace.join("profile/home/AppData").join(name)
                    );
                    assert!(Path::new(path).is_dir());
                }
                if kind == DesktopHarnessKind::Zed {
                    let mut shell = Command::new("powershell.exe");
                    shell.args(["-NoProfile", "-NonInteractive", "-Command", "if ([Environment]::GetFolderPath('LocalApplicationData') -ne $env:LOCALAPPDATA) { exit 3 }; if ([Environment]::GetFolderPath('ApplicationData') -ne $env:APPDATA) { exit 4 }"])
                        .envs(command.as_std().get_envs().filter_map(|(key, value)| value.map(|value| (key, value))))
                        .kill_on_drop(true);
                    let status = tokio::time::timeout(Duration::from_secs(15), shell.status())
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(
                        status.success(),
                        "the shell must resolve private AppData folders: {status}"
                    );
                }
            }
        }
    }

    #[test]
    fn zed_profile_keeps_privacy_and_readable_text_without_overwriting_settings() {
        let directory = tempfile::tempdir().unwrap();
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::Zed,
            nan_harness: directory.path().join("nanh"),
            nan_harness_sha256: "a".repeat(64),
            executable: directory.path().join("app"),
            workspace: directory.path().join("workspace"),
            model: "qwen3.6".into(),
            live: false,
            probe_index: None,
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        prepare_zed_profile(&spec).unwrap();
        let path = spec.workspace.join("profile/zed/config/settings.json");
        let original = std::fs::read(&path).unwrap();
        let settings: Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(settings["auto_update"], false);
        assert_eq!(settings["telemetry"]["metrics"], false);
        assert_eq!(settings["telemetry"]["diagnostics"], false);
        assert_eq!(settings["agent_ui_font_size"], 18);
        assert_eq!(settings["agent_buffer_font_size"], 16);
        assert!(prepare_zed_profile(&spec).is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
        let semantic = ProbeSpec {
            workspace: directory.path().join("semantic-workspace"),
            verification: crate::cli::VerificationPolicy::SemanticOnly,
            ..spec
        };
        prepare_zed_profile(&semantic).unwrap();
        let settings: Value = serde_json::from_slice(
            &std::fs::read(semantic.workspace.join("profile/zed/config/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["ui_font_size"], 16);
        assert_eq!(settings["agent_ui_font_size"], 16);
        assert_eq!(settings["auto_update"], false);
        assert_eq!(settings["telemetry"]["metrics"], false);
        assert_eq!(settings["telemetry"]["diagnostics"], false);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_probes_refuse_ambient_profiles_before_running_binaries_or_creating_state() {
        let directory = tempfile::tempdir().unwrap();
        for kind in DesktopHarnessKind::ALL {
            let spec = ProbeSpec {
                kind,
                nan_harness: directory.path().join("missing-nanh.exe"),
                nan_harness_sha256: "a".repeat(64),
                executable: directory.path().join("missing-app.exe"),
                workspace: directory.path().join(kind.to_string()),
                model: "synthetic-model".into(),
                live: false,
                probe_index: None,
                session: crate::cli::SessionMode::PrivateProfile,
                verification: crate::cli::VerificationPolicy::default(),
                launch_wrapper: None,
            };
            let result = execute(&spec).await.result;
            assert_eq!(result.status, Status::Blocked);
            assert_eq!(result.reason, Some(Reason::IsolationUnavailable));
            assert!(result.steps.is_empty());
            assert!(!spec.workspace.exists());
        }
    }

    #[tokio::test]
    async fn recovery_preserves_changed_binary_and_workspace_identity() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = crate::journal::Journal::create(parent.path()).unwrap();
        let root = journal.reserve("zed-desktop-deterministic-0").unwrap();
        let binary = parent.path().join("nanh");
        std::fs::write(&binary, "original binary").unwrap();
        let mut spec = ProbeSpec {
            kind: DesktopHarnessKind::Zed,
            nan_harness: binary.clone(),
            nan_harness_sha256: binary_digest(&binary).unwrap(),
            executable: parent.path().join("zed"),
            workspace: root.join("workspace"),
            model: "qwen3.6".into(),
            live: false,
            probe_index: None,
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        std::fs::write(root.join("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        std::fs::write(&binary, "changed binary").unwrap();
        assert!(
            recover_pending(&mut journal)
                .await
                .unwrap_err()
                .contains("identity changed")
        );
        assert!(root.join("spec.json").is_file());
        assert!(journal.cleanup(false).is_err());

        spec.nan_harness_sha256 = binary_digest(&binary).unwrap();
        spec.workspace = parent.path().join("user-owned");
        std::fs::write(root.join("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        assert!(
            recover_pending(&mut journal)
                .await
                .unwrap_err()
                .contains("identity changed")
        );
        assert_eq!(journal.pending_names().len(), 1);
    }

    #[tokio::test]
    async fn recovery_does_not_seal_an_interrupted_installer() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = crate::journal::Journal::create(parent.path()).unwrap();
        let root = journal.reserve("installation").unwrap();
        std::fs::write(root.join("partial-download"), "partial bytes").unwrap();
        recover_pending(&mut journal).await.unwrap();
        assert!(journal.cleanup(false).is_err());
        assert!(root.join("partial-download").is_file());
    }

    #[test]
    fn tools_are_allowlisted_and_only_read_the_fixture() {
        let fixture = Path::new("/private/fixture/read-target.txt");
        assert!(
            select_read_tool(
                &[json!({"tools":[{"function":{"name":"delete_everything"}}]})],
                fixture
            )
            .is_none()
        );
        let (name, input) =
            select_read_tool(&[json!({"tools":[{"function":{"name":"Read"}}]})], fixture).unwrap();
        assert_eq!(name, "Read");
        assert_eq!(input["file_path"], fixture.to_str().unwrap());
    }

    #[test]
    fn probe_specs_without_a_wrapper_keep_their_encoding() {
        let spec = ProbeSpec {
            kind: DesktopHarnessKind::ChatGpt,
            nan_harness: "/synthetic/nanh".into(),
            nan_harness_sha256: "a".repeat(64),
            executable: "/synthetic/app".into(),
            workspace: "/synthetic/workspace".into(),
            model: "qwen3.6".into(),
            live: false,
            probe_index: None,
            session: crate::cli::SessionMode::PrivateProfile,
            verification: crate::cli::VerificationPolicy::default(),
            launch_wrapper: None,
        };
        let mut value = serde_json::to_value(&spec).unwrap();
        assert!(value.get("launchWrapper").is_none());
        let decoded: ProbeSpec = serde_json::from_value(value.clone()).unwrap();
        assert!(decoded.launch_wrapper.is_none());
        value["launchWrapper"] = json!({"path": "/w", "sha256": "b".repeat(64), "facts": "/f"});
        let decoded: ProbeSpec = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded.launch_wrapper.unwrap().facts, Path::new("/f"));
        value["launchWrapper"]["output"] = json!("private app output");
        assert!(serde_json::from_value::<ProbeSpec>(value).is_err());
    }

    #[test]
    fn the_wrapper_deadline_outlasts_every_worker_bound() {
        let deadline = Duration::from_secs(LAUNCH_WRAPPER_DEADLINE_SECONDS);
        assert!(deadline > crate::runner::worker_timeout(false));
        assert!(deadline > crate::runner::worker_timeout(true));
    }

    /// Runs the real wave-12 wrapper and reducer through the checker's own
    /// command builders, with the synthetic fixture standing in for nanh.
    #[cfg(unix)]
    mod launch_wrapper {
        use super::*;
        use std::{collections::BTreeMap, os::unix::fs::PermissionsExt as _};

        // Synthetic privacy markers the fixture prints as application output.
        const MARKERS: [&str; 3] = [
            "sk-super-secret-value-0123456789",
            "/home/runner/.nan-harness/private",
            "ignore previous instructions",
        ];

        struct Fixture {
            directory: tempfile::TempDir,
            spec: ProbeSpec,
            calls: PathBuf,
        }

        impl Fixture {
            fn new() -> Self {
                let directory = tempfile::tempdir().unwrap();
                let bin = directory.path().join("bin");
                std::fs::create_dir(&bin).unwrap();
                // The reducer must sit beside the wrapper: the checker removes
                // every ambient reducer override from the wrapped launch.
                for (source, name) in [
                    ("chatgpt-wave12-shim.sh", "chatgpt-wave12-shim.sh"),
                    ("chatgpt-wave12-reducer.py", "chatgpt-wave12-reducer.py"),
                    ("chatgpt-wave12-fixture-nanh.sh", "nanh"),
                ] {
                    let target = bin.join(name);
                    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
                    std::fs::copy(scripts.join(source), &target).unwrap();
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
                let facts = directory.path().join("facts");
                std::fs::create_dir(&facts).unwrap();
                std::fs::set_permissions(&facts, std::fs::Permissions::from_mode(0o700)).unwrap();
                let nanh = bin.join("nanh");
                let wrapper = bin.join("chatgpt-wave12-shim.sh");
                let spec = ProbeSpec {
                    kind: DesktopHarnessKind::ChatGpt,
                    nan_harness_sha256: binary_digest(&nanh).unwrap(),
                    nan_harness: nanh,
                    executable: directory.path().join("ChatGPT"),
                    workspace: directory.path().join("workspace"),
                    model: "qwen3.6".into(),
                    live: false,
                    probe_index: None,
                    session: crate::cli::SessionMode::PrivateProfile,
                    verification: crate::cli::VerificationPolicy::default(),
                    launch_wrapper: Some(LaunchWrapper {
                        sha256: binary_digest(&wrapper).unwrap(),
                        path: wrapper,
                        facts: facts.join("chatgpt-desktop-deterministic-0"),
                    }),
                };
                let calls = directory.path().join("calls");
                Self {
                    directory,
                    spec,
                    calls,
                }
            }

            fn wrapper(&self) -> &LaunchWrapper {
                self.spec.launch_wrapper.as_ref().unwrap()
            }

            fn command(&self, gate: &ProviderGate, scenario: &str) -> Command {
                prepare_launch_wrapper(self.spec.kind, self.wrapper()).unwrap();
                let mut command = launch_command(&self.spec, gate).unwrap();
                command
                    .env("FIXTURE_SCENARIO", scenario)
                    .env("FIXTURE_CALLS", &self.calls);
                command
            }

            async fn observe(&self, gate: &ProviderGate, scenario: &str) -> LaunchExit {
                let mut command = self.command(gate, scenario);
                let status = tokio::time::timeout(Duration::from_secs(30), command.status())
                    .await
                    .unwrap()
                    .unwrap();
                launcher_exit(status)
            }

            /// The facts must pass the reducer's own closed validator.
            fn facts(&self) -> Value {
                let path = self.wrapper().facts.join("startup-facts.json");
                let reducer = self.directory.path().join("bin/chatgpt-wave12-reducer.py");
                let status = std::process::Command::new("python3")
                    .arg(reducer)
                    .args(["validate", "--facts"])
                    .arg(&path)
                    .status()
                    .unwrap();
                assert!(status.success(), "the facts failed their own validator");
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
            }

            /// Nothing the wrapper wrote carries app output or a local path.
            fn assert_private(&self) {
                let location = self.directory.path().to_string_lossy().into_owned();
                for entry in std::fs::read_dir(&self.wrapper().facts).unwrap() {
                    let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
                    for marker in MARKERS.into_iter().chain([location.as_str()]) {
                        assert!(!text.contains(marker), "facts carried private data");
                    }
                }
            }
        }

        async fn synthetic_gate() -> ProviderGate {
            ProviderGate::start(
                "http://127.0.0.1:1/v1",
                Zeroizing::new("synthetic-provider-key".into()),
                false,
                "fixture-marker",
            )
            .await
            .unwrap()
        }

        fn arguments(command: &Command) -> Vec<String> {
            command
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        }

        fn wave12_environment(command: &Command) -> BTreeMap<String, Option<String>> {
            command
                .as_std()
                .get_envs()
                .filter(|(name, _)| name.to_string_lossy().starts_with("WAVE12_"))
                .map(|(name, value)| {
                    let value = value.map(|value| value.to_string_lossy().into_owned());
                    (name.to_string_lossy().into_owned(), value)
                })
                .collect()
        }

        #[tokio::test]
        async fn only_the_chatgpt_launch_runs_through_the_bound_wrapper() {
            let fixture = Fixture::new();
            let gate = synthetic_gate().await;
            let wrapped = launch_command(&fixture.spec, &gate).unwrap();
            let direct_spec = ProbeSpec {
                launch_wrapper: None,
                ..fixture.spec.clone()
            };
            let direct = launch_command(&direct_spec, &gate).unwrap();
            assert_eq!(
                direct.as_std().get_program(),
                fixture.spec.nan_harness.as_os_str()
            );
            assert!(wave12_environment(&direct).is_empty());
            assert_eq!(
                wrapped.as_std().get_program(),
                fixture.wrapper().path.as_os_str()
            );
            assert_eq!(arguments(&wrapped), arguments(&direct));
            let bound = wave12_environment(&wrapped);
            let text = |path: &Path| Some(path.to_string_lossy().into_owned());
            assert_eq!(bound["WAVE12_REAL_NANH"], text(&fixture.spec.nan_harness));
            assert_eq!(
                bound["WAVE12_REAL_SHA256"].as_deref(),
                Some(fixture.spec.nan_harness_sha256.as_str())
            );
            assert_eq!(bound["WAVE12_FACTS_DIR"], text(&fixture.wrapper().facts));
            assert_eq!(bound["WAVE12_DEADLINE_S"].as_deref(), Some("300"));
            for removed in [
                "WAVE12_REDUCER",
                "WAVE12_PYTHON",
                "WAVE12_GRACE_S",
                "WAVE12_MAX_BYTES",
                "WAVE12_MAX_LINE_BYTES",
                "WAVE12_MAX_LINES",
            ] {
                assert_eq!(bound[removed], None, "{removed} must not be inherited");
            }

            // The real wrapper appends --debug to exactly the checker's vector.
            let exit = fixture.observe(&gate, "quiet-exit0").await;
            assert_eq!(exit, LaunchExit::Code(0));
            let mut expected = arguments(&direct);
            expected.extend(["--debug".into(), "key=present".into()]);
            let calls = std::fs::read_to_string(&fixture.calls).unwrap();
            assert_eq!(calls.lines().collect::<Vec<_>>(), expected);
            let facts = fixture.facts();
            assert_eq!(facts["observation"], "complete");
            assert_eq!(facts["classification"], "no-signature");
            assert_eq!(facts["bounds"]["deadlineSeconds"], 300);
            let identity = &facts["identity"];
            assert_eq!(identity["realNanhSha256"], fixture.spec.nan_harness_sha256);
            assert_eq!(identity["shimSha256"], fixture.wrapper().sha256);
            let reducer = fixture
                .directory
                .path()
                .join("bin/chatgpt-wave12-reducer.py");
            assert_eq!(identity["reducerSha256"], binary_digest(&reducer).unwrap());
            fixture.assert_private();
        }

        #[tokio::test]
        async fn wrapped_launches_keep_the_launcher_disposition_and_claim_no_cause() {
            let terminated = nix::sys::signal::Signal::SIGTERM as i32;
            for (scenario, exit, classification) in [
                (
                    "startup-error-sandbox",
                    LaunchExit::Code(1),
                    "no-usable-sandbox",
                ),
                ("unknown-only", LaunchExit::Code(1), "no-signature"),
                ("exit-reserved", LaunchExit::Code(78), "no-signature"),
                ("signaled", LaunchExit::Signal(terminated), "no-signature"),
            ] {
                let fixture = Fixture::new();
                let observed = fixture.observe(&synthetic_gate().await, scenario).await;
                assert_eq!(observed, exit, "{scenario}");
                let facts = fixture.facts();
                assert_eq!(facts["classification"], classification, "{scenario}");
                assert_eq!(facts["failure"], "none", "{scenario}");
                fixture.assert_private();
            }
        }

        #[tokio::test]
        async fn the_checker_stop_and_the_wrapper_deadline_end_a_wrapped_launch() {
            let fixture = Fixture::new();
            let gate = synthetic_gate().await;
            let mut process =
                ProbeProcess::spawn(fixture.command(&gate, "stall-until-terminated")).unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            let group = process.id().and_then(|pid| i32::try_from(pid).ok());
            assert_eq!(stop(&mut process, None, group).await, Ok(()));
            let status = process.try_wait().unwrap().map(launcher_exit);
            assert_eq!(status, Some(LaunchExit::Code(143)));
            let facts = fixture.facts();
            assert_eq!(facts["observation"], "cancelled");
            // The checker signals the whole group. The child can reap before the
            // wrapper forwards that same signal; both observations are truthful.
            assert!(matches!(
                facts["stopAction"].as_str(),
                Some("forwarded" | "attempted")
            ));
            assert_eq!(facts["classification"], "observation-failed");
            let calls = std::fs::read_to_string(&fixture.calls).unwrap();
            assert!(calls.lines().any(|line| line == "term-seen"));
            fixture.assert_private();

            let fixture = Fixture::new();
            let mut command = fixture.command(&gate, "stall-until-terminated");
            command.env("WAVE12_DEADLINE_S", "1");
            let status = tokio::time::timeout(Duration::from_secs(30), command.status())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(launcher_exit(status), LaunchExit::Code(143));
            let facts = fixture.facts();
            assert_eq!(facts["observation"], "timeout");
            assert_eq!(facts["stopAction"], "forwarded");
            assert_eq!(facts["classification"], "observation-failed");
            fixture.assert_private();
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn stop_terminates_a_probe_owned_process_tree() {
            let script =
                "trap 'exit 0' TERM; (trap '' TERM; printf R; while :; do sleep 1; done) & wait";
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg(script)
                .process_group(0)
                .stdout(Stdio::piped())
                .kill_on_drop(true);
            let mut process = ProbeProcess::spawn(command).unwrap();
            let mut ready = [0];
            tokio::time::timeout(
                Duration::from_secs(5),
                process.take_stdout().unwrap().read_exact(&mut ready),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(ready, [b'R']);
            let group = process.id().and_then(|pid| i32::try_from(pid).ok());
            let mut sentinel = Command::new("sleep")
                .arg("30")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            assert_eq!(stop(&mut process, None, group).await, Ok(()));
            assert!(group.is_some_and(group_absent));
            assert!(sentinel.try_wait().unwrap().is_none());
            let _ = sentinel.kill().await;
        }

        #[tokio::test]
        async fn help_and_restoration_bypass_the_wrapper() {
            let fixture = Fixture::new();
            prepare_launch_wrapper(fixture.spec.kind, fixture.wrapper()).unwrap();
            let help = endpoint_help_command(&fixture.spec);
            let restoration = restore_command(&fixture.spec).unwrap();
            for command in [&help, &restoration] {
                assert_eq!(
                    command.as_std().get_program(),
                    fixture.spec.nan_harness.as_os_str()
                );
                assert!(wave12_environment(command).is_empty());
            }
            assert_eq!(require_endpoint_override(&fixture.spec).await, Ok(()));
            assert_eq!(restore(&fixture.spec).await, Ok(()));
            let facts = std::fs::read_dir(&fixture.wrapper().facts).unwrap();
            assert_eq!(facts.count(), 0, "a direct call produced wrapper facts");
        }

        #[tokio::test]
        async fn endpoint_help_distinguishes_missing_capabilities_from_valid_help() {
            use std::os::unix::fs::PermissionsExt as _;

            let directory = tempfile::tempdir().unwrap();
            let nanh = directory.path().join("nanh");
            std::fs::write(&nanh, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&nanh, std::fs::Permissions::from_mode(0o700)).unwrap();
            for (mode, expected) in [
                ("provider", Err(Reason::HarnessCapabilityUnavailable)),
                ("user", Err(Reason::HarnessCapabilityUnavailable)),
                ("failed", Err(Reason::HarnessCapabilityUnavailable)),
                ("valid", Ok(())),
            ] {
                let help = match mode {
                    "provider" => "echo --user-data-dir",
                    "user" => "echo --provider-base-url",
                    "valid" => "echo --provider-base-url --user-data-dir",
                    "failed" => "echo --provider-base-url --user-data-dir; exit 1",
                    _ => unreachable!(),
                };
                std::fs::write(&nanh, format!("#!/bin/sh\n{help}\n")).unwrap();
                let mut fixture = Fixture::new();
                fixture.spec.kind = if mode == "provider" {
                    DesktopHarnessKind::ChatGpt
                } else {
                    DesktopHarnessKind::Zed
                };
                fixture.spec.nan_harness = nanh.clone();
                fixture.spec.nan_harness_sha256 = binary_digest(&nanh).unwrap();
                fixture.spec.launch_wrapper = None;
                assert_eq!(
                    require_endpoint_override(&fixture.spec).await,
                    expected,
                    "{mode}"
                );
            }
        }

        #[tokio::test]
        async fn missing_or_mismatched_bindings_refuse_before_any_process_runs() {
            for case in ["mismatched", "missing", "other-app", "reused-facts"] {
                let mut fixture = Fixture::new();
                let wrapper = fixture.spec.launch_wrapper.as_mut().unwrap();
                let (status, reason) = match case {
                    "mismatched" => {
                        wrapper.sha256 = "0".repeat(64);
                        (Status::Failed, Reason::InstallationUnreadable)
                    }
                    "missing" => {
                        wrapper.path = fixture.directory.path().join("absent-wrapper");
                        (Status::Failed, Reason::InstallationUnreadable)
                    }
                    "other-app" => {
                        fixture.spec.kind = DesktopHarnessKind::Zed;
                        (Status::Failed, Reason::InstallationUnreadable)
                    }
                    _ => {
                        nan_harness_private_fs::create_private_dir(&wrapper.facts).unwrap();
                        (Status::Blocked, Reason::IsolationUnavailable)
                    }
                };
                let outcome = execute(&fixture.spec).await;
                assert_eq!(outcome.result.status, status, "{case}");
                assert_eq!(outcome.result.reason, Some(reason), "{case}");
                assert!(outcome.result.steps.is_empty(), "{case}");
                assert!(outcome.launch_exit.is_none(), "{case}");
                // The launch needs the workspace; no step before it may run.
                assert!(!fixture.spec.workspace.exists(), "{case}");
                let facts = &fixture.wrapper().facts;
                assert_eq!(facts.exists(), case == "reused-facts", "{case}");
                if facts.exists() {
                    assert_eq!(std::fs::read_dir(facts).unwrap().count(), 0);
                }
            }
        }
    }
}
