//! Diagnostic-only fresh-profile comparisons; no result qualifies a release.
use super::{PublishedConformanceRunner, environment};
use crate::conformance::arguments::RunKind;
use crate::conformance::constants::SENTINEL_MARKER;
use crate::conformance::registry::harness_registration;
use crate::scripted_provider::{ProviderScenario, ScriptedProvider};
use crate::terminal::{TerminalCommand, TerminalOutput};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const RUNTIME_NAMES: &[&str] = &[
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "PSModulePath",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "NUMBER_OF_PROCESSORS",
    "USERDOMAIN",
    "USERNAME",
];

pub(super) fn restore_runtime_environment(mut command: TerminalCommand) -> TerminalCommand {
    for name in RUNTIME_NAMES {
        if let Some((_, value)) =
            std::env::vars_os().find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
        {
            command = command.env(*name, value);
        }
    }
    command
}

fn capture(output: Option<&TerminalOutput>) -> Value {
    output.map_or_else(
        || json!({"status": "process-error"}),
        |output| {
            json!({
                "status": if output.status.success() { "passed" } else { "nonzero-exit" },
                "exitCode": output.status.code(),
                "stdoutBytes": output.stdout.len(),
                "stderrBytes": output.stderr.len(),
                "marker": output.stdout.contains(SENTINEL_MARKER),
            })
        },
    )
}

async fn direct(executable: &Path, argument: &str) -> Value {
    let workspace = ConformanceWorkspace::create().expect("private workspace required");
    let command = TerminalCommand::new(executable, workspace.path())
        .clear_environment()
        .args([argument])
        .env("CI", "1")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", workspace.path().join("home"));
    #[cfg(windows)]
    let command =
        environment::apply(command, workspace.path()).expect("private environment required");
    #[cfg(not(windows))]
    let command = environment::apply(command, workspace.path());
    let output = command.timeout(Duration::from_secs(30)).run().await;
    json!({
        "case": if argument == "--version" { "fresh-version" } else { "fresh-help" },
        "process": capture(output.as_ref().ok()),
        "versionVerified": argument == "--version" && output.as_ref().is_ok_and(|output|
            output.status.success() && output.stdout.trim() == "2.5.3"),
    })
}

async fn sentinel(binary: &Path, runtime_environment: bool) -> Value {
    let workspace = ConformanceWorkspace::create().expect("private workspace required");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(SENTINEL_MARKER))
        .await
        .expect("private fixture provider required");
    let registration = *harness_registration(HarnessKind::KimiCode).expect("registered Kimi");
    let mut runner = PublishedConformanceRunner::new(binary, HarnessKind::KimiCode);
    runner.kimi_runtime_environment = runtime_environment;
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Sentinel,
            SENTINEL_MARKER,
        )
        .await;
    let request_count = provider.chat_requests().len().min(10_000);
    let completed = provider.completed();
    let bounded = provider.recording_bounded();
    let shutdown = provider.shutdown().await.is_ok();
    json!({
        "case": if runtime_environment { "sentinel-runtime-environment" } else { "sentinel-cleared-environment" },
        "process": capture(output.as_ref().ok()),
        "provider": {"requestCount": request_count, "completed": completed,
            "bounded": bounded, "shutdown": shutdown},
    })
}

fn checkpoint(destination: &Path, records: &[Value]) {
    let parent = destination.parent().expect("absolute output path required");
    let mut file = tempfile::NamedTempFile::new_in(parent).expect("private checkpoint required");
    serde_json::to_writer(file.as_file_mut(), &json!({
        "schemaVersion": 1, "diagnosticsOnly": true, "expectedVersion": "2.5.3", "records": records,
    })).expect("closed report serialization");
    file.persist(destination).expect("atomic closed checkpoint");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires an explicitly installed Kimi 2.5.3 and source CLI on Windows"]
async fn native_kimi_startup_boundaries() {
    let required_path = |name| {
        let path = PathBuf::from(std::env::var_os(name).expect("diagnostic path required"));
        assert!(path.is_absolute(), "absolute diagnostic path required");
        path
    };
    let binary = required_path("NAN_KIMI_STARTUP_BINARY");
    let executable = required_path("NAN_KIMI_STARTUP_EXECUTABLE");
    let destination = required_path("NAN_KIMI_STARTUP_OUTPUT");
    assert!(
        binary.is_file() && executable.is_file(),
        "staged executables required"
    );
    assert!(!destination.exists(), "fresh diagnostic output required");
    let mut records = Vec::new();
    checkpoint(&destination, &records);
    for argument in ["--version", "--help"] {
        records.push(direct(&executable, argument).await);
        checkpoint(&destination, &records);
        assert!(
            records.last().is_some_and(clean_process),
            "process cleanup must be proven"
        );
    }
    // Preplanned paired fresh sessions, with no credentials restored from the host.
    for runtime_environment in [false, true, false, true] {
        records.push(sentinel(&binary, runtime_environment).await);
        checkpoint(&destination, &records);
        assert!(
            records.last().is_some_and(clean_process),
            "process cleanup must be proven"
        );
        assert_eq!(
            records.last().expect("recorded case")["provider"]["shutdown"],
            true,
            "provider cleanup required"
        );
    }
}

fn clean_process(record: &Value) -> bool {
    matches!(
        record["process"]["status"].as_str(),
        Some("passed" | "nonzero-exit")
    )
}

#[test]
fn capture_never_exports_child_text() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt as _;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt as _;
    let output = TerminalOutput {
        status: std::process::ExitStatus::from_raw(0),
        stdout: format!("{SENTINEL_MARKER} private-prompt"),
        stderr: "private-secret".to_owned(),
    };
    let observed = capture(Some(&output));
    assert_eq!(observed["marker"], true);
    assert_eq!(observed["status"], "passed");
    assert_eq!(observed["stderrBytes"], 14);
    assert!(!observed.to_string().contains("private"));
    assert_eq!(capture(None), json!({"status": "process-error"}));
}

#[test]
fn terminal_errors_stop_the_comparison_even_when_files_can_be_removed() {
    assert!(!clean_process(&json!({"process": capture(None)})));
    assert!(!clean_process(&json!({})));
    assert!(clean_process(
        &json!({"process": {"status": "nonzero-exit"}})
    ));
}
