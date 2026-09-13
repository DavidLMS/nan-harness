//! Each native accessibility probe runs in a bounded child process.

use crate::{
    gui::{ComposerFailure, Gui, GuiFailure},
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
use tokio::{
    io::AsyncReadExt as _,
    process::{Child, Command},
};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_wrapper: Option<LaunchWrapper>,
}

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
// The wrapper's reducer refuses a deadline above ten minutes.
const _: () = assert!(LAUNCH_WRAPPER_DEADLINE_SECONDS <= 600);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkerOutcome {
    pub(crate) result: ProbeResult,
    pub(crate) launch_exit: Option<LaunchExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_failure: Option<crate::diagnostics::LaunchFailure>,
    pub(crate) cleanup: Option<CleanupDiagnostic>,
    #[serde(default)]
    pub(crate) composer: Vec<ComposerFailure>,
    #[serde(default)]
    pub(crate) gui_acquisition: Option<crate::diagnostics::GuiAcquisitionDiagnostic>,
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
    #[cfg(unix)]
    Signal(i32),
    Unknown,
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
    failure: Option<crate::diagnostics::LaunchFailure>,
}

impl LaunchObservation {
    fn capture(process: &mut Child, spec: &ProbeSpec) -> Self {
        Self {
            exit: process.try_wait().ok().flatten().map(launcher_exit),
            failure: read_child_launch_failure(spec),
        }
    }
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
        launch_failure: launch_observation.failure,
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
    // Windows known folders and credential stores follow the OS identity, not
    // HOME. Only an explicitly declared disposable hosted VM may use that account.
    if !spec.session.available()
        || (cfg!(windows) && spec.session != crate::cli::SessionMode::GithubHosted)
    {
        return Err(Reason::IsolationUnavailable);
    }
    if binary_digest(&spec.nan_harness)? != spec.nan_harness_sha256 {
        return Err(Reason::InstallationUnreadable);
    }
    if let Some(wrapper) = &spec.launch_wrapper {
        prepare_launch_wrapper(spec.kind, wrapper)?;
        *diagnostic_allowed = true;
    }
    Gui::ensure_absent(spec.kind).map_err(|failure| failure.reason)?;
    require_endpoint_override(spec).await?;
    create_private_dir_all(&spec.workspace).map_err(|_| Reason::IsolationUnavailable)?;
    prepare_zed_profile(spec)?;
    let marker = visual_marker("NAN CHECK READ")?;
    let fixture = spec.workspace.join("read-target.txt");
    open_private_new(&fixture)
        .and_then(|mut file| file.write_all(marker.as_bytes()))
        .map_err(|_| Reason::IsolationUnavailable)?;
    let final_marker = visual_marker("NAN CHECK RESPONSE")?;
    let inventory = ScriptedProvider::start(ProviderScenario::inventory(&final_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
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
    let gate = ProviderGate::start(upstream, key, spec.live, &marker)
        .await
        .map_err(|()| Reason::ProviderFailed)?;
    let mut process = launch(spec, &gate).map_err(|(reason, failure)| {
        launch_observation.failure = Some(failure);
        reason
    })?;
    #[cfg(unix)]
    let process_group = process.id().and_then(|pid| i32::try_from(pid).ok());
    #[cfg(windows)]
    let process_group = None;
    let gui = Gui::wait(spec.kind, &mut process);
    if gui.is_err() {
        *launch_observation = LaunchObservation::capture(&mut process, spec);
    }
    let outcome = match &gui {
        Ok(gui) => {
            result.steps.push(CheckStep::Launched);
            if let Err(failure) = gui.prepare_conversation() {
                result.gui_stage = Some(failure.stage);
                Err(failure.reason)
            } else if spec.live {
                live(
                    gui,
                    spec,
                    &gate,
                    &fixture,
                    &marker,
                    result,
                    composer_observations,
                )
            } else {
                deterministic(
                    gui,
                    &inventory,
                    &gate,
                    &fixture,
                    &final_marker,
                    result,
                    composer_observations,
                )
                .await
            }
        }
        Err((reason, acquisition_stage)) => {
            *gui_acquisition = Some(crate::diagnostics::GuiAcquisitionDiagnostic {
                stage: *acquisition_stage,
                error_category: crate::gui::error_category(*reason),
                reason: *reason,
            });
            Err(*reason)
        }
    };
    finish_scenario(
        spec,
        &mut process,
        gui.as_ref().ok(),
        process_group,
        outcome,
        &gate,
        diagnostic,
    )
    .await
}

fn read_child_launch_failure(spec: &ProbeSpec) -> Option<crate::diagnostics::LaunchFailure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Record {
        schema_version: u8,
        failure: crate::diagnostics::LaunchFailure,
    }

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
    let record = serde_json::from_slice::<Record>(&bytes).ok()?;
    (record.schema_version == 1).then_some(record.failure)
}

async fn finish_scenario(
    spec: &ProbeSpec,
    process: &mut Child,
    gui: Option<&Gui>,
    process_group: Option<ProcessGroupId>,
    outcome: Result<(), Reason>,
    gate: &ProviderGate,
    diagnostic: &mut Option<CleanupDiagnostic>,
) -> Result<(), Reason> {
    if let Err(failure) = stop(process, gui, process_group).await {
        *diagnostic = Some(CleanupDiagnostic {
            stage: CleanupStage::Stop,
            original_reason: outcome.as_ref().err().copied(),
            reason: Reason::CleanupFailed,
            absence: None,
            stop: Some(failure.diagnostic),
        });
        return Err(Reason::CleanupFailed);
    }
    record_absence(
        Gui::ensure_absent(spec.kind),
        CleanupStage::AbsenceAfterStop,
        outcome.err(),
        diagnostic,
    )?;
    record_cleanup(
        restore(spec).await,
        CleanupStage::Restore,
        outcome.err(),
        diagnostic,
    )?;
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

fn record_cleanup(
    outcome: Result<(), Reason>,
    stage: CleanupStage,
    original_reason: Option<Reason>,
    diagnostic: &mut Option<CleanupDiagnostic>,
) -> Result<(), Reason> {
    outcome.map_err(|reason| {
        *diagnostic = Some(CleanupDiagnostic {
            stage,
            original_reason,
            reason,
            absence: None,
            stop: None,
        });
        Reason::CleanupFailed
    })
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
            let function = tool.get("function")?;
            let name = function.get("name")?.as_str()?;
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
    let mut command = Command::new(program);
    command
        .arg(spec.kind.to_string())
        // Upstream apps discover global skills and credentials outside their
        // config directory. Redirect their home too, never the parent process.
        .env("HOME", profile.join("home"))
        .env("USERPROFILE", profile.join("home"))
        .env("CODEX_HOME", profile.join("home").join(".codex"))
        .env("NAN_HARNESS_CONFIG_DIR", profile.join("nanh"))
        // The probe owns its provider budget. A detached per-profile coordinator
        // would outlive the app and keep Windows recovery files locked.
        .env("NAN_HARNESS_INTERNAL_DISABLE_COORDINATOR", "1")
        .env("XDG_CONFIG_HOME", profile.join("config"))
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
    // Keep the launched desktop app and any helper descendants in a probe-owned
    // group. Cleanup must be able to prove and remove the whole tree without
    // signalling an unrelated application in the runner's process group.
    #[cfg(unix)]
    command.process_group(0);
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

fn prepare_zed_profile(spec: &ProbeSpec) -> Result<(), Reason> {
    if spec.kind != DesktopHarnessKind::Zed {
        return Ok(());
    }
    let directory = spec.workspace.join("profile").join("zed").join("config");
    create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
    // Never let a probe update an existing app or send its diagnostics elsewhere.
    // Use legible agent text in this private profile; exact OCR checks stay unchanged.
    open_private_new(&directory.join("settings.json"))
        .and_then(|mut file| {
            file.write_all(
                br#"{"auto_update":false,"telemetry":{"metrics":false,"diagnostics":false},"agent_ui_font_size":18,"agent_buffer_font_size":16}"#,
            )
        })
        .map_err(|_| Reason::IsolationUnavailable)
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
    command
        .args([
            "--provider-base-url",
            &gate.base_url,
            "--model",
            &spec.model,
            "--executable",
        ])
        .arg(&spec.executable)
        .env("NAN_API_KEY", gate.session_token())
        .env(
            "NAN_NATIVE_LAUNCH_DIAGNOSTIC",
            spec.workspace.join("native-launch-diagnostic.json"),
        );
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
) -> Result<Child, (Reason, crate::diagnostics::LaunchFailure)> {
    let mut command = launch_command(spec, gate)
        .map_err(|reason| (reason, crate::diagnostics::LaunchFailure::LaunchSetup))?;
    command.spawn().map_err(|_| {
        (
            Reason::UnsupportedVersion,
            crate::diagnostics::LaunchFailure::LauncherSpawn,
        )
    })
}

fn restore_command(spec: &ProbeSpec) -> Result<Command, Reason> {
    let mut command = isolated_command(spec, &spec.nan_harness)?;
    command.arg("--restore");
    Ok(command)
}

async fn restore(spec: &ProbeSpec) -> Result<(), Reason> {
    let mut command = restore_command(spec)?;
    let status = tokio::time::timeout(Duration::from_secs(30), command.status())
        .await
        .map_err(|_| Reason::CleanupFailed)?
        .map_err(|_| Reason::CleanupFailed)?;
    if status.success() {
        Ok(())
    } else {
        Err(Reason::CleanupFailed)
    }
}

#[cfg(unix)]
type ProcessGroupId = i32;
#[cfg(windows)]
type ProcessGroupId = u32;

const TERM_GRACE: Duration = Duration::from_secs(2);

async fn stop(
    process: &mut Child,
    gui: Option<&Gui>,
    process_group: Option<ProcessGroupId>,
) -> Result<(), StopFailure> {
    #[cfg(windows)]
    debug_assert!(process_group.is_none());
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
    match tokio::time::timeout(Duration::from_secs(10), process.wait()).await {
        Ok(Ok(_)) => diagnostic.initial_wait.outcome = StopWaitOutcome::Reaped,
        Ok(Err(error)) => {
            diagnostic.initial_wait.outcome = StopWaitOutcome::Failed;
            diagnostic.initial_wait.os_error = error.raw_os_error();
        }
        Err(_) => {}
    }
    if diagnostic.initial_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(unix)]
        if process_group.is_none_or(group_absent) {
            return Ok(());
        }
        #[cfg(windows)]
        return Ok(());
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
    match tokio::time::timeout(TERM_GRACE, process.wait()).await {
        Ok(Ok(_)) => diagnostic.grace_wait.outcome = StopWaitOutcome::Reaped,
        Ok(Err(error)) => {
            diagnostic.grace_wait.outcome = StopWaitOutcome::Failed;
            diagnostic.grace_wait.os_error = error.raw_os_error();
        }
        Err(_) => diagnostic.grace_wait.outcome = StopWaitOutcome::TimedOut,
    }
    if diagnostic.grace_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(unix)]
        if process_group.is_none_or(group_absent) {
            return Ok(());
        }
        #[cfg(windows)]
        return Ok(());
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
    match tokio::time::timeout(TERM_GRACE, process.wait()).await {
        Ok(Ok(_)) => diagnostic.final_wait.outcome = StopWaitOutcome::Reaped,
        Ok(Err(error)) => {
            diagnostic.final_wait.outcome = StopWaitOutcome::Failed;
            diagnostic.final_wait.os_error = error.raw_os_error();
        }
        Err(_) => diagnostic.final_wait.outcome = StopWaitOutcome::TimedOut,
    }
    if diagnostic.final_wait.outcome == StopWaitOutcome::Reaped {
        #[cfg(windows)]
        return Ok(());
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
    use super::*;

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
    }

    #[test]
    fn child_launch_failure_read_is_bounded_and_works_without_wrapper() {
        use crate::diagnostics::LaunchFailure;
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
            launch_wrapper: None,
        };
        let path = spec.workspace.join("native-launch-diagnostic.json");
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
                &path,
                serde_json::to_vec(&json!({"schemaVersion": 1, "failure": failure})).unwrap(),
            )
            .unwrap();
            assert_eq!(read_child_launch_failure(&spec), Some(expected));
        }
        std::fs::write(&path, vec![b'x'; 1025]).unwrap();
        assert_eq!(read_child_launch_failure(&spec), None);
        std::fs::write(&path, br#"{"schemaVersion":1,"failure":"private"}"#).unwrap();
        assert_eq!(read_child_launch_failure(&spec), None);
    }

    #[test]
    fn composer_diagnostic_is_closed_and_operation_specific() {
        let diagnostic = ComposerDiagnostic {
            schema_version: 1,
            observations: vec![ComposerFailure {
                operation: crate::gui::ComposerOperation::TypeText,
                error_category: crate::gui::ComposerErrorCategory::ActionUnsupported,
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
    fn cleanup_failures_preserve_the_original_reason_and_exact_stage() {
        for stage in [
            CleanupStage::Stop,
            CleanupStage::AbsenceAfterStop,
            CleanupStage::Restore,
            CleanupStage::AbsenceAfterRestore,
        ] {
            let mut diagnostic = None;
            assert_eq!(
                record_cleanup(
                    Ok(()),
                    stage,
                    Some(Reason::SelectorNotMatched),
                    &mut diagnostic
                ),
                Ok(())
            );
            assert!(diagnostic.is_none());
            assert_eq!(
                record_cleanup(
                    Err(Reason::AlreadyRunning),
                    stage,
                    Some(Reason::SelectorNotMatched),
                    &mut diagnostic
                ),
                Err(Reason::CleanupFailed)
            );
            assert_eq!(
                diagnostic,
                Some(CleanupDiagnostic {
                    stage,
                    original_reason: Some(Reason::SelectorNotMatched),
                    reason: Reason::AlreadyRunning,
                    absence: None,
                    stop: None,
                })
            );
        }
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
        extra["stop"]["graceWait"]["pid"] = json!(34748758294u64);
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
                "kill": {"outcome": "failed", "osError": 2147483648u64},
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
                launch_failure: None,
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
            let mut process = fixture
                .command(&gate, "stall-until-terminated")
                .spawn()
                .unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            let group = process.id().and_then(|pid| i32::try_from(pid).ok());
            assert_eq!(stop(&mut process, None, group).await, Ok(()));
            let status = process.try_wait().unwrap().map(launcher_exit);
            assert_eq!(status, Some(LaunchExit::Code(143)));
            let facts = fixture.facts();
            assert_eq!(facts["observation"], "cancelled");
            assert_eq!(facts["stopAction"], "forwarded");
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
            let mut process = command.spawn().unwrap();
            let mut ready = [0];
            tokio::time::timeout(
                Duration::from_secs(5),
                process.stdout.take().unwrap().read_exact(&mut ready),
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

        #[cfg(windows)]
        #[tokio::test]
        async fn windows_stop_reaps_a_synthetic_child_after_handle_kill() {
            let mut process = Command::new("cmd.exe")
                .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(stop(&mut process, None, None).await, Ok(()));
            assert!(process.try_wait().unwrap().is_some());
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
