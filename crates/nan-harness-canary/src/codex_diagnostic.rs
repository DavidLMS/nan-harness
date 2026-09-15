//! Credential-free, one-pass Codex process diagnostics.

use crate::app::CodexDiagnosticArgs;
use nan_harness_test_support::terminal::{
    CaptureMode, CleanupStage, DiagnosticOutput, ProcessEvent, ReaderOutcome, ScanState,
    SurvivorScan, TerminalCommand,
};
use serde::Serialize;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;

const IDS: &[&str] = &[
    "install.version_resolution",
    "launch.direct.pipe",
    "launch.direct.private_file",
    "launch.supervised.pipe",
    "launch.supervised.private_file",
    "probe.inventory",
    "probe.tool_round_trip",
    "probe.sentinel",
    "lifecycle.normal_exit",
    "lifecycle.timeout",
    "lifecycle.cancel",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticReport<'a> {
    schema_version: u8,
    harness: &'static str,
    cases: &'a [Case],
    overall: Overall,
    duration_milliseconds: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    status: Status,
    reason: Reason,
    duration_milliseconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence: Option<Evidence>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    None,
    UnsafePrerequisite,
    Unavailable,
    LaunchFailed,
    VersionFailed,
    InventoryFailed,
    ToolRoundTripFailed,
    SentinelFailed,
    NormalExitFailed,
    TimeoutFailed,
    CancelFailed,
    DeadlineExceeded,
    CleanupFailed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Evidence {
    event: &'static str,
    capture: &'static str,
    root_exit: RootExit,
    stdout: StreamEvidence,
    stderr: StreamEvidence,
    termination: TerminationEvidence,
    cleanup: CleanupEvidence,
    marker: MarkerEvidence,
    provider: ProviderEvidence,
    readers: Readers,
    after_cleanup: AfterCleanup,
    survivors: Survivors,
}

/// Independent reader state at the capture deadline, per stream.
#[derive(Debug, Serialize)]
struct Readers {
    stdout: &'static str,
    stderr: &'static str,
}

/// End of file observed only after the owned tree was terminated.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AfterCleanup {
    stdout: DelayedEof,
    stderr: DelayedEof,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DelayedEof {
    eof: &'static str,
    at_milliseconds: Option<u64>,
}

/// Live descendants of the launched root before and after owned-tree termination.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Survivors {
    at_failure: SurvivorEvidence,
    residual: SurvivorEvidence,
}

#[derive(Debug, Serialize)]
struct SurvivorEvidence {
    scan: &'static str,
    names: Vec<String>,
    count: u32,
}

fn reader_name(reader: ReaderOutcome) -> &'static str {
    match reader {
        ReaderOutcome::Eof => "eof",
        ReaderOutcome::Open => "open",
        ReaderOutcome::Error => "error",
    }
}

fn delayed_eof(at: Option<Duration>) -> DelayedEof {
    DelayedEof {
        eof: if at.is_some() { "observed" } else { "unknown" },
        at_milliseconds: at.map(milliseconds),
    }
}

fn scan_name(state: ScanState) -> &'static str {
    match state {
        ScanState::NotNeeded => "not_needed",
        ScanState::Available => "available",
        ScanState::Unavailable => "unavailable",
    }
}

fn survivor_evidence(scan: &SurvivorScan) -> SurvivorEvidence {
    SurvivorEvidence {
        scan: scan_name(scan.state),
        names: scan.names.clone(),
        count: scan.count,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RootExit {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<i64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamEvidence {
    eof: &'static str,
    at_milliseconds: Option<u64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminationEvidence {
    before: TerminationStep,
    after: TerminationStep,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminationStep {
    attempted: bool,
    result: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanupEvidence {
    stage: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_error_code: Option<u32>,
}
#[derive(Debug, Serialize)]
struct MarkerEvidence {
    observed: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderEvidence {
    requests: u32,
    round_trip: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Overall {
    executed: u32,
    passed: u32,
    failed: u32,
    blocked: u32,
}

#[derive(Debug, Error)]
pub(crate) enum DiagnosticError {
    #[error("could not write diagnostic report")]
    Report(#[source] std::io::Error),
    #[error("could not serialize diagnostic report")]
    Serialize(#[source] serde_json::Error),
}

pub(crate) async fn run(arguments: &CodexDiagnosticArgs) -> Result<(), DiagnosticError> {
    let started = Instant::now();
    let total = Duration::from_millis(arguments.total_deadline_ms.max(1));
    let mut cases = IDS
        .iter()
        .map(|id| blocked(id, Reason::DeadlineExceeded))
        .collect::<Vec<_>>();
    checkpoint(arguments, &cases, started)?;
    for (index, id) in IDS.iter().enumerate() {
        let remaining = total.saturating_sub(started.elapsed());
        let native_only = id.starts_with("install.") || id.starts_with("launch.direct.");
        let available = id.starts_with("lifecycle.")
            || (arguments.codex.is_file() && (native_only || arguments.nan_harness.is_file()));
        cases[index] = if !available {
            blocked(id, Reason::UnsafePrerequisite)
        } else if remaining < Duration::from_millis(500) {
            blocked(id, Reason::DeadlineExceeded)
        } else {
            run_case(
                id,
                arguments,
                remaining.min(Duration::from_millis(arguments.case_deadline_ms)),
            )
            .await
        };
        checkpoint(arguments, &cases, started)?;
    }
    Ok(())
}

fn checkpoint(
    arguments: &CodexDiagnosticArgs,
    cases: &[Case],
    started: Instant,
) -> Result<(), DiagnosticError> {
    let count = |status| {
        cases
            .iter()
            .filter(|case| std::mem::discriminant(&case.status) == std::mem::discriminant(&status))
            .fold(0_u32, |count, _| count + 1)
    };
    let overall = Overall {
        executed: count(Status::Passed) + count(Status::Failed),
        passed: count(Status::Passed),
        failed: count(Status::Failed),
        blocked: count(Status::Blocked),
    };
    let report = DiagnosticReport {
        schema_version: 1,
        harness: "codex",
        cases,
        overall,
        duration_milliseconds: milliseconds(started.elapsed()),
    };
    let encoded = serde_json::to_vec_pretty(&report).map_err(DiagnosticError::Serialize)?;
    let parent = arguments
        .report
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(DiagnosticError::Report)?;
    pending
        .write_all(&encoded)
        .map_err(DiagnosticError::Report)?;
    pending
        .persist(&arguments.report)
        .map_err(|error| DiagnosticError::Report(error.error))?;
    Ok(())
}

fn blocked(id: &str, reason: Reason) -> Case {
    Case {
        id: id.to_owned(),
        status: Status::Blocked,
        reason,
        duration_milliseconds: 0,
        evidence: None,
    }
}

async fn run_case(id: &str, arguments: &CodexDiagnosticArgs, limit: Duration) -> Case {
    let started = Instant::now();
    let Ok(workspace) = tempfile::tempdir() else {
        return blocked(id, Reason::UnsafePrerequisite);
    };
    if nan_harness_private_fs::restrict_path(
        workspace.path(),
        nan_harness_private_fs::PrivatePathKind::Directory,
    )
    .is_err()
    {
        return blocked(id, Reason::UnsafePrerequisite);
    }
    if id.starts_with("lifecycle.") {
        return lifecycle_case(id, workspace.path(), limit).await;
    }
    if id.starts_with("install.") {
        let Some(command) = isolated_command(&arguments.codex, workspace.path()) else {
            return blocked(id, Reason::UnsafePrerequisite);
        };
        let output = command
            .args(["--version"])
            .timeout(limit)
            .diagnose(CaptureMode::Pipe, None)
            .await;
        let passed = output.status.is_some_and(|status| status.success())
            && output
                .stdout
                .split_whitespace()
                .any(|word| word == arguments.expected_version);
        return finish(id, &output, passed, Reason::VersionFailed);
    }
    probe(
        id,
        arguments,
        workspace.path(),
        limit.saturating_sub(started.elapsed()),
    )
    .await
}

async fn probe(
    id: &str,
    arguments: &CodexDiagnosticArgs,
    workspace: &Path,
    limit: Duration,
) -> Case {
    use nan_harness_test_support::scripted_provider::ScriptedProvider;
    let started = Instant::now();
    let tool = id == "probe.tool_round_trip";
    let marker = "NAN_CODEX_DIAGNOSTIC_OK";
    let scenario = probe_scenario(tool, marker);
    let Ok(provider) = ScriptedProvider::start(scenario).await else {
        return blocked(id, Reason::Unavailable);
    };
    let direct = id.starts_with("launch.direct.");
    let mut bridge = if direct {
        direct_bridge(provider.base_url()).await
    } else {
        None
    };
    if direct && bridge.is_none() {
        return blocked(id, Reason::Unavailable);
    }
    let Some(mut command) = isolated_command(
        if direct {
            &arguments.codex
        } else {
            &arguments.nan_harness
        },
        workspace,
    ) else {
        return blocked(id, Reason::UnsafePrerequisite);
    };
    command = configure_probe(
        command,
        bridge.as_ref(),
        arguments,
        provider.base_url(),
        tool,
        marker,
        workspace,
    );
    let mode = if id.ends_with("private_file") {
        CaptureMode::PrivateFile
    } else {
        CaptureMode::Pipe
    };
    let output = command
        .timeout(
            limit
                .saturating_sub(started.elapsed())
                .saturating_sub(Duration::from_millis(200)),
        )
        .diagnose(mode, None)
        .await;
    let requests = provider.chat_requests();
    let marker_seen = output.stdout.contains(marker);
    let (inventory_seen, round_trip) = verify_exchange(&requests, workspace, tool);
    let mut passed = output.event == ProcessEvent::Exited
        && output.status.is_some_and(|status| status.success())
        && marker_seen
        && provider.completed()
        && provider.recording_bounded()
        && inventory_seen
        && (!tool || round_trip);
    if id == "probe.sentinel" {
        passed &= !output.stdout.contains("NH-BRIDGE-") && !output.stderr.contains("NH-BRIDGE-");
    }
    if let Some(bridge) = &mut bridge {
        bridge.shutdown();
        passed &= tokio::time::timeout(limit.saturating_sub(started.elapsed()) / 2, bridge.wait())
            .await
            .is_ok_and(|result| result.is_ok());
    }
    passed &= tokio::time::timeout(limit.saturating_sub(started.elapsed()), provider.shutdown())
        .await
        .is_ok_and(|result| result.is_ok());
    let mut case = finish(
        id,
        &output,
        passed,
        if tool {
            Reason::ToolRoundTripFailed
        } else if id == "probe.inventory" {
            Reason::InventoryFailed
        } else if id == "probe.sentinel" {
            Reason::SentinelFailed
        } else {
            Reason::LaunchFailed
        },
    );
    if let Some(evidence) = &mut case.evidence {
        evidence.marker.observed = marker_seen;
        evidence.provider.requests = u32::try_from(requests.len()).unwrap_or(u32::MAX);
        evidence.provider.round_trip = round_trip;
    }
    case.duration_milliseconds = milliseconds(started.elapsed());
    case
}

fn isolated_command(program: &Path, workspace: &Path) -> Option<TerminalCommand> {
    let home = workspace.join("home");
    for path in [
        &home,
        &home.join(".codex"),
        &home.join("AppData/Roaming"),
        &home.join("AppData/Local"),
        &workspace.join("tmp"),
    ] {
        nan_harness_private_fs::create_private_dir_all(path).ok()?;
    }
    let catalog =
        nan_harness_bridge::CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
            .ok()?
            .api_response();
    nan_harness_private_fs::open_private_new(&home.join("catalog.json"))
        .ok()?
        .write_all(serde_json::to_string(&catalog).ok()?.as_bytes())
        .ok()?;
    let mut command = TerminalCommand::new(program, workspace)
        .clear_environment()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CODEX_HOME", home.join(".codex"))
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"))
        .env("TEMP", workspace.join("tmp"))
        .env("TMP", workspace.join("tmp"))
        .env("NAN_HARNESS_CONFIG_DIR", workspace.join("nan-config"))
        .env("NAN_API_KEY", "nan-diagnostic-placeholder")
        .env("CI", "1")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1");
    for name in ["SystemRoot", "WINDIR", "ComSpec", "PATHEXT"] {
        if let Some((_, value)) =
            std::env::vars_os().find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
        {
            command = command.env(name, value);
        }
    }
    Some(command)
}

async fn direct_bridge(base_url: &str) -> Option<nan_harness_bridge::RunningBridge> {
    use nan_harness_bridge::{CodexModelCatalog, ResponsesBridgeConfig};
    use std::sync::Arc;
    let key = Arc::new(nan_harness_core::SecretValue::new("nan-diagnostic-placeholder").ok()?);
    nan_harness_bridge::spawn_responses(
        tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?,
        ResponsesBridgeConfig {
            launch_id: "diagnostic".to_owned(),
            provider_base_url: base_url.to_owned(),
            models: CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6").ok()?,
            provider_api_key: Arc::clone(&key),
            session_token: key,
            web_search_enabled: false,
            search_config: None,
            session_max_tokens: None,
        },
    )
    .ok()
}

async fn lifecycle_case(id: &str, workspace: &Path, limit: Duration) -> Case {
    let normal = id.ends_with("normal_exit");
    let (program, args) = if cfg!(windows) {
        (
            "cmd",
            if normal {
                vec!["/d", "/c", "echo NAN_CODEX_DIAGNOSTIC_OK"]
            } else {
                vec!["/d", "/c", "ping 127.0.0.1 -n 120 > nul"]
            },
        )
    } else {
        (
            "/bin/sh",
            if normal {
                vec!["-c", "printf NAN_CODEX_DIAGNOSTIC_OK"]
            } else {
                vec!["-c", "sleep 120"]
            },
        )
    };
    let cancel = id.ends_with("cancel");
    let Some(command) = isolated_command(Path::new(program), workspace) else {
        return blocked(id, Reason::UnsafePrerequisite);
    };
    let output = command
        .args(args)
        .timeout(limit)
        .diagnose(
            CaptureMode::Pipe,
            cancel.then_some(Duration::from_millis(100)),
        )
        .await;
    let expected = if normal {
        ProcessEvent::Exited
    } else if cancel {
        ProcessEvent::Cancelled
    } else {
        ProcessEvent::TimedOut
    };
    let passed = output.event == expected
        && if normal {
            output.status.is_some_and(|status| status.success())
                && output.stdout.contains("NAN_CODEX_DIAGNOSTIC_OK")
        } else {
            output.cleanup.is_some() && output.cleanup == Some(true)
        };
    finish(
        id,
        &output,
        passed,
        if normal {
            Reason::NormalExitFailed
        } else if cancel {
            Reason::CancelFailed
        } else {
            Reason::TimeoutFailed
        },
    )
}

fn finish(id: &str, output: &DiagnosticOutput, passed: bool, reason: Reason) -> Case {
    let elapsed = milliseconds(output.duration);
    let cleanup_ok = output.cleanup.is_none() || output.cleanup == Some(true);
    Case {
        id: id.to_owned(),
        status: if passed && cleanup_ok {
            Status::Passed
        } else {
            Status::Failed
        },
        reason: if !cleanup_ok {
            Reason::CleanupFailed
        } else if passed {
            Reason::None
        } else {
            reason
        },
        duration_milliseconds: elapsed,
        evidence: Some(evidence(output)),
    }
}

/// Projects the closed evidence record for one observed case.
fn evidence(output: &DiagnosticOutput) -> Evidence {
    let code = output
        .status
        .and_then(|status| status.code())
        .map(i64::from);
    Evidence {
        event: match output.event {
            ProcessEvent::Exited => "exited",
            ProcessEvent::TimedOut => "timed_out",
            ProcessEvent::Cancelled => "cancelled",
            ProcessEvent::Failed => "failed",
        },
        capture: if output.capture_mode == CaptureMode::Pipe {
            "pipe"
        } else {
            "private_file"
        },
        root_exit: RootExit {
            kind: if code.is_some() {
                "code"
            } else {
                "unavailable"
            },
            value: code,
        },
        stdout: StreamEvidence {
            eof: if output.stdout_eof {
                "observed"
            } else {
                "unknown"
            },
            at_milliseconds: output.stdout_eof_after.map(milliseconds),
        },
        stderr: StreamEvidence {
            eof: if output.stderr_eof {
                "observed"
            } else {
                "unknown"
            },
            at_milliseconds: output.stderr_eof_after.map(milliseconds),
        },
        termination: TerminationEvidence {
            before: TerminationStep {
                attempted: false,
                result: "not_needed",
            },
            after: TerminationStep {
                attempted: output.cleanup.is_some(),
                result: if output.cleanup.is_none() {
                    "not_needed"
                } else if output.cleanup == Some(true) {
                    "succeeded"
                } else {
                    "unknown"
                },
            },
        },
        cleanup: CleanupEvidence {
            stage: match output.cleanup_stage {
                Some(CleanupStage::Terminate) => "terminate",
                Some(CleanupStage::Wait) => "wait",
                Some(CleanupStage::WaitTimeout) => "wait_timeout",
                Some(CleanupStage::CaptureTimeout) if output.event == ProcessEvent::Failed => {
                    "capture_timeout"
                }
                None | Some(CleanupStage::CaptureTimeout) => "none",
            },
            os_error_code: output.os_error_code,
        },
        marker: MarkerEvidence { observed: false },
        provider: ProviderEvidence {
            requests: 0,
            round_trip: false,
        },
        readers: Readers {
            stdout: reader_name(output.stdout_reader),
            stderr: reader_name(output.stderr_reader),
        },
        after_cleanup: AfterCleanup {
            stdout: delayed_eof(output.stdout_eof_after_cleanup),
            stderr: delayed_eof(output.stderr_eof_after_cleanup),
        },
        survivors: Survivors {
            at_failure: survivor_evidence(&output.survivors_at_failure),
            residual: survivor_evidence(&output.survivors_residual),
        },
    }
}

fn configure_probe(
    mut command: TerminalCommand,
    bridge: Option<&nan_harness_bridge::RunningBridge>,
    arguments: &CodexDiagnosticArgs,
    base_url: &str,
    tool: bool,
    marker: &str,
    workspace: &Path,
) -> TerminalCommand {
    if let Some(bridge) = bridge {
        command = command.args([
            "-c".to_owned(), "model=\"qwen3.6\"".to_owned(),
            "-c".to_owned(), "model_reasoning_effort=\"high\"".to_owned(),
            "-c".to_owned(), format!("model_catalog_json={}",serde_json::to_string(&workspace.join("home/catalog.json").to_string_lossy()).unwrap_or_default()),
            "-c".to_owned(), "features.responses_websockets=false".to_owned(),
            "-c".to_owned(), "features.responses_websockets_v2=false".to_owned(),
            "-c".to_owned(), "model_provider=\"diagnostic\"".to_owned(),
            "-c".to_owned(), format!("model_providers.diagnostic={{name=\"diagnostic\",base_url=\"{}/v1\",env_key=\"NAN_API_KEY\",wire_api=\"responses\",requires_openai_auth=false,request_max_retries=0,stream_max_retries=0}}",bridge.base_url())
        ]);
    } else {
        command = command
            .args([
                "codex",
                "--model",
                "qwen3.6",
                "--provider-base-url",
                base_url,
                "--executable",
            ])
            .args([arguments.codex.as_os_str()])
            .args(["--"]);
    }
    let prompt = if tool {
        format!("Use exec_command once, wait for its result, then reply exactly {marker}.")
    } else {
        format!("Reply exactly {marker} without using tools.")
    };
    command = command.args([
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--dangerously-bypass-approvals-and-sandbox",
        "--json",
        &prompt,
    ]);

    command
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn verify_exchange(requests: &[serde_json::Value], workspace: &Path, tool: bool) -> (bool, bool) {
    let inventory_seen = requests.iter().any(|request| {
        request
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|tools| !tools.is_empty())
    });
    let round_trip =
        tool && requests.iter().any(|request| {
            request
                .get("messages")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"))
        }) && fs::read_to_string(workspace.join("tool-output.txt"))
            .is_ok_and(|text| text == "NAN_TOOL_OK");

    (inventory_seen, round_trip)
}

fn probe_scenario(
    tool: bool,
    marker: &str,
) -> nan_harness_test_support::scripted_provider::ProviderScenario {
    use nan_harness_test_support::scripted_provider::ProviderScenario;
    let tool_command = if cfg!(windows) {
        "Set-Content -LiteralPath tool-output.txt -Value NAN_TOOL_OK -NoNewline"
    } else {
        "printf NAN_TOOL_OK > tool-output.txt"
    };
    if tool {
        ProviderScenario::tool(
            "exec_command",
            serde_json::json!({"cmd":tool_command}),
            marker,
        )
    } else {
        ProviderScenario::inventory(marker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires an explicitly selected local Codex executable"]
    async fn native_direct_probe_uses_the_synthetic_bridge() {
        use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
        let root = tempfile::tempdir().unwrap();
        let executable = std::env::var_os("NAN_CODEX_DIAGNOSTIC_TEST_EXECUTABLE").unwrap();
        let args = CodexDiagnosticArgs {
            nan_harness: "unused".into(),
            codex: executable.into(),
            expected_version: "unused".to_owned(),
            report: root.path().join("unused"),
            case_deadline_ms: 10000,
            total_deadline_ms: 10000,
        };
        let provider = ScriptedProvider::start(ProviderScenario::inventory("DIAGNOSTIC_OK"))
            .await
            .unwrap();
        let bridge = direct_bridge(provider.base_url()).await.unwrap();
        let command = configure_probe(
            isolated_command(&args.codex, root.path()).unwrap(),
            Some(&bridge),
            &args,
            provider.base_url(),
            false,
            "DIAGNOSTIC_OK",
            root.path(),
        );
        let output = command
            .timeout(Duration::from_secs(10))
            .diagnose(CaptureMode::Pipe, None)
            .await;
        assert!(
            output.status.is_some_and(|status| status.success()),
            "synthetic fixture stdout={} stderr={}",
            output.stdout,
            output.stderr
        );
        assert!(provider.completed());
    }

    #[tokio::test]
    async fn missing_prerequisite_checkpoints_all_cases_without_payloads() {
        let root = tempfile::tempdir().unwrap();
        let args = CodexDiagnosticArgs {
            nan_harness: root.path().join("missing"),
            codex: root.path().join("missing-codex"),
            expected_version: "0.0.0".to_owned(),
            report: root.path().join("report.json"),
            case_deadline_ms: 500,
            total_deadline_ms: 3000,
        };
        run(&args).await.unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&args.report).unwrap()).unwrap();
        assert_eq!(report["cases"].as_array().unwrap().len(), 11);
        assert_eq!(report["overall"]["blocked"], 8);
        assert_eq!(report["overall"]["passed"], 3);
        assert!(!report.to_string().contains(root.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn lifecycle_reports_distinct_events_and_verified_cleanup() {
        for (id, event) in [
            ("lifecycle.timeout", "timed_out"),
            ("lifecycle.cancel", "cancelled"),
            ("lifecycle.normal_exit", "exited"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let case = lifecycle_case(id, root.path(), Duration::from_secs(1)).await;
            assert!(matches!(case.status, Status::Passed));
            assert_eq!(case.evidence.unwrap().event, event);
        }
    }

    /// Exercises the real supervisor with a harness double that leaves a descendant holding the
    /// inherited standard streams, so the closed attribution facts are verified without depending
    /// on a live Codex conversation. Linux and macOS validate the attribution and the delayed end
    /// of file; Windows additionally requires the supervisor to terminate its owned descendants
    /// before it exits.
    ///
    /// The workflow runs this ignored test explicitly with both binaries:
    ///
    /// ```text
    /// NAN_CODEX_DIAGNOSTIC_TEST_NAN_HARNESS=<nan-harness> \
    /// NAN_CODEX_DIAGNOSTIC_TEST_HARNESS_FIXTURE=<codex-harness-fixture> \
    /// cargo test --locked -p nan-harness-canary --all-features -- --ignored --exact \
    ///   codex_diagnostic::tests::native_supervised_launch_attributes_a_leaked_descendant --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "requires an explicitly selected nan-harness binary and harness fixture"]
    async fn native_supervised_launch_attributes_a_leaked_descendant() {
        use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
        let nan_harness = std::env::var_os("NAN_CODEX_DIAGNOSTIC_TEST_NAN_HARNESS")
            .expect("NAN_CODEX_DIAGNOSTIC_TEST_NAN_HARNESS must select the nan-harness binary");
        let fixture = std::env::var_os("NAN_CODEX_DIAGNOSTIC_TEST_HARNESS_FIXTURE")
            .expect("NAN_CODEX_DIAGNOSTIC_TEST_HARNESS_FIXTURE must select the fixture binary");
        let marker = "NAN_CODEX_DIAGNOSTIC_OK";
        let root = tempfile::tempdir().unwrap();
        nan_harness_private_fs::restrict_path(
            root.path(),
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .unwrap();
        let args = CodexDiagnosticArgs {
            nan_harness: nan_harness.into(),
            codex: fixture.into(),
            expected_version: "0.0.0-diagnostic".to_owned(),
            report: root.path().join("unused.json"),
            case_deadline_ms: 20_000,
            total_deadline_ms: 20_000,
        };
        let provider = ScriptedProvider::start(ProviderScenario::inventory(marker))
            .await
            .unwrap();
        let command = isolated_command(&args.nan_harness, root.path()).unwrap();
        let command = command.env("NAN_CODEX_HARNESS_FIXTURE_MODE", "descendant");
        let command = configure_probe(
            command,
            None,
            &args,
            provider.base_url(),
            false,
            marker,
            root.path(),
        );
        let output = command
            .timeout(Duration::from_secs(20))
            .diagnose(CaptureMode::Pipe, None)
            .await;
        let _ = provider.shutdown().await;
        println!(
            "supervised stdio evidence: status={:?} eof={}/{} readers={:?}/{:?} after_cleanup={:?}/{:?} at_failure={:?} residual={:?}",
            output.status.and_then(|status| status.code()),
            output.stdout_eof,
            output.stderr_eof,
            output.stdout_reader,
            output.stderr_reader,
            output.stdout_eof_after_cleanup,
            output.stderr_eof_after_cleanup,
            output.survivors_at_failure,
            output.survivors_residual,
        );
        assert!(
            output.stdout.contains(marker),
            "the fixture harness should have run: {output:?}"
        );
        if !output.stdout_eof || !output.stderr_eof {
            // An open pipe must be explained by live descendants, and terminating the owned tree
            // must release it; anything else means the surviving writer escaped ownership.
            assert_eq!(output.survivors_at_failure.state, ScanState::Available);
            assert!(
                output.survivors_at_failure.count >= 1
                    && !output.survivors_at_failure.names.is_empty(),
                "an open pipe must be attributed to live descendants: {:?}",
                output.survivors_at_failure
            );
            assert!(
                output.stdout_eof_after_cleanup.is_some()
                    && output.stderr_eof_after_cleanup.is_some(),
                "terminating the owned tree must release the inherited pipes: {output:?}"
            );
        }
        assert_eq!(output.survivors_residual.state, ScanState::Available);
        assert_eq!(
            output.survivors_residual.count, 0,
            "the owned tree must not outlive the launch: {:?}",
            output.survivors_residual
        );
        if cfg!(windows) {
            assert!(
                output.stdout_eof && output.stderr_eof,
                "the Windows supervisor must terminate its owned descendants before it exits: {output:?}"
            );
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn successful_version_only_stubs_cannot_pass_probes_or_hide_later_cases() {
        let root = tempfile::tempdir().unwrap();
        let args = CodexDiagnosticArgs {
            nan_harness: "/bin/echo".into(),
            codex: "/bin/echo".into(),
            expected_version: "--version".to_owned(),
            report: root.path().join("report.json"),
            case_deadline_ms: 500,
            total_deadline_ms: 5000,
        };
        run(&args).await.unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&args.report).unwrap()).unwrap();
        let cases = report["cases"].as_array().unwrap();
        for case in &cases[1..8] {
            assert_eq!(case["status"], "failed", "{}", case["id"]);
        }
        assert_eq!(cases[10]["status"], "passed");
    }
}
