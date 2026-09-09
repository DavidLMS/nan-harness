//! Each native accessibility probe runs in a bounded child process.

use crate::{
    gui::Gui,
    provider::ProviderGate,
    report::{CheckStep, ProbeResult, Reason, Status},
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProbeSpec {
    pub(crate) kind: DesktopHarnessKind,
    pub(crate) nan_harness: PathBuf,
    pub(crate) nan_harness_sha256: String,
    pub(crate) executable: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) model: String,
    pub(crate) live: bool,
    #[serde(default)]
    pub(crate) session: crate::cli::SessionMode,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkerOutcome {
    pub(crate) result: ProbeResult,
    pub(crate) cleanup: Option<CleanupDiagnostic>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CleanupStage {
    Stop,
    AbsenceAfterStop,
    Restore,
    AbsenceAfterRestore,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CleanupDiagnostic {
    stage: CleanupStage,
    original_reason: Option<Reason>,
    reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    absence: Option<crate::gui::AbsenceStage>,
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

async fn execute(spec: &ProbeSpec) -> WorkerOutcome {
    let started = Instant::now();
    let mut result = ProbeResult::blocked(Reason::NotRun);
    let mut cleanup = None;
    let outcome = scenario(spec, &mut result, &mut cleanup).await;
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
    WorkerOutcome { result, cleanup }
}

async fn scenario(
    spec: &ProbeSpec,
    result: &mut ProbeResult,
    diagnostic: &mut Option<CleanupDiagnostic>,
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
    let mut process = launch(spec, &gate)?;
    let gui = Gui::wait(spec.kind, &mut process);
    let outcome = match &gui {
        Ok(gui) => {
            result.steps.push(CheckStep::Launched);
            if let Err(reason) = gui.prepare_conversation() {
                Err(reason)
            } else if spec.live {
                live(gui, spec, &gate, &fixture, &marker, result)
            } else {
                deterministic(gui, &inventory, &gate, &fixture, &final_marker, result).await
            }
        }
        Err(reason) => Err(*reason),
    };
    let closed = stop(&mut process, gui.as_ref().ok()).await;
    record_cleanup(closed, CleanupStage::Stop, outcome.err(), diagnostic)?;
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
) -> Result<(), Reason> {
    let prompt = format!(
        "Use your file-reading tool to read {}. In your final response write NAN_CHECK_FINAL: immediately followed by the exact file contents. Do not guess or answer before the tool succeeds.",
        fixture.display()
    );
    result.record_input(gui.submit(&prompt)?);
    result.steps.push(CheckStep::InputSubmitted);
    result.record_response(
        gui.wait_text(&format!("NAN_CHECK_FINAL:{marker}"), Duration::from_mins(2))?,
    );
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
) -> Result<(), Reason> {
    result.record_input(gui.submit("Check this connection")?);
    result.steps.push(CheckStep::InputSubmitted);
    let response = gui.wait_text(marker, Duration::from_secs(30));
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
    result.record_input(gui.submit("Read read-target.txt using your file tool.")?);
    result.record_response(gui.wait_text(&tool_marker, Duration::from_secs(30))?);
    if !tool.completed() || !tool.recording_bounded() || !gate.tool_verified() {
        return Err(Reason::ToolMismatch);
    }
    result.steps.push(CheckStep::ToolVerified);
    gate.fail_next_scenario(true);
    result.record_input(gui.submit("Check the expected provider failure")?);
    result.record_response(gui.wait_text("NAN_CHECK_EXPECTED_FAILURE", Duration::from_secs(20))?);
    if !gate.failure_observed() {
        return Err(Reason::ProviderFailed);
    }
    gate.fail_next_scenario(false);
    let recovery_marker = visual_marker("NAN CHECK RECOVERED")?;
    let recovered = ScriptedProvider::start(ProviderScenario::inventory(&recovery_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    gate.use_upstream(recovered.base_url());
    result.record_input(gui.submit("Try the connection again")?);
    result.record_response(gui.wait_text(&recovery_marker, Duration::from_secs(30))?);
    result.steps.push(CheckStep::ErrorRecovered);
    Ok(())
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

fn isolated_command(spec: &ProbeSpec) -> Result<Command, Reason> {
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
    let mut command = Command::new(&spec.nan_harness);
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

async fn require_endpoint_override(spec: &ProbeSpec) -> Result<(), Reason> {
    let mut command = Command::new(&spec.nan_harness);
    command
        .arg(spec.kind.to_string())
        .arg("--help")
        .env_remove("NAN_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| Reason::UnsupportedVersion)?;
    let stdout = child.stdout.take().ok_or(Reason::UnsupportedVersion)?;
    let operation = async {
        let mut bytes = Vec::new();
        stdout
            .take(65537)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Reason::UnsupportedVersion)?;
        let status = child.wait().await.map_err(|_| Reason::UnsupportedVersion)?;
        if !status.success()
            || bytes.len() > 65536
            || !String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .any(|word| word == "--provider-base-url")
        {
            return Err(Reason::UnsupportedVersion);
        }
        if spec.kind == DesktopHarnessKind::Zed
            && !String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .any(|word| word == "--user-data-dir")
        {
            return Err(Reason::UnsupportedVersion);
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| Reason::UnsupportedVersion)?
}

fn launch_command(spec: &ProbeSpec, gate: &ProviderGate) -> Result<Command, Reason> {
    let mut command = isolated_command(spec)?;
    command
        .args([
            "--provider-base-url",
            &gate.base_url,
            "--model",
            &spec.model,
            "--executable",
        ])
        .arg(&spec.executable)
        .env("NAN_API_KEY", gate.session_token());
    if spec.kind == DesktopHarnessKind::Zed {
        command.arg(&spec.workspace);
    }
    Ok(command)
}

fn launch(spec: &ProbeSpec, gate: &ProviderGate) -> Result<Child, Reason> {
    let mut command = launch_command(spec, gate)?;
    command.spawn().map_err(|_| Reason::UnsupportedVersion)
}

async fn restore(spec: &ProbeSpec) -> Result<(), Reason> {
    let mut command = isolated_command(spec)?;
    command.arg("--restore");
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

async fn stop(process: &mut Child, gui: Option<&Gui>) -> Result<(), Reason> {
    if let Some(gui) = gui {
        let _ = gui.quit();
    }
    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(10), process.wait()).await {
        return Ok(());
    }
    #[cfg(unix)]
    if let Some(pid) = process.id().and_then(|pid| i32::try_from(pid).ok()) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(20), process.wait()).await {
        return Ok(());
    }
    let _ = process.kill().await;
    Err(Reason::CleanupFailed)
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
                cleanup,
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
                session: crate::cli::SessionMode::PrivateProfile,
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
            session: crate::cli::SessionMode::PrivateProfile,
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
                session: crate::cli::SessionMode::PrivateProfile,
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
            session: crate::cli::SessionMode::PrivateProfile,
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
}
