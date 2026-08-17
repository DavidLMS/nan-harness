#![cfg(unix)]

use nan_harness_test_support::terminal::{TerminalCommand, TerminalOutput};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires OpenClaw, network access, and NAN_API_KEY"]
async fn openclaw_completes_a_real_read_tool_round_trip() {
    let workspace = live_workspace("OPENCLAW_LIVE_READ_OK");
    let target = workspace.path().join("read-target.txt");
    let prompt = format!(
        "Use the read tool to read '{}'. Do not answer before using the tool. After it succeeds, reply exactly OPENCLAW_LIVE_OK.",
        target.display()
    );
    let output = live_command(
        workspace.path(),
        [
            "run",
            "openclaw",
            "--model",
            "qwen3.6",
            "--",
            "agent",
            "--local",
            "--session-id",
            "nan-harness-live-openclaw",
            "--message",
            &prompt,
            "--json",
        ],
    )
    .run()
    .await
    .expect("OpenClaw live conformance should complete before the timeout");
    assert_success(&output);
    let report: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("OpenClaw should return JSON: {error}\n{}", output.stdout));
    assert_eq!(
        report.pointer("/meta/toolSummary/failures"),
        Some(&serde_json::Value::from(0))
    );
    assert!(output.stdout.contains("\"read\""), "{}", output.stdout);
    assert!(
        output.stdout.contains("OPENCLAW_LIVE_OK"),
        "{}",
        output.stdout
    );
}

#[tokio::test]
#[ignore = "requires Cline, network access, and NAN_API_KEY"]
async fn cline_completes_a_real_read_tool_round_trip() {
    let workspace = live_workspace("CLINE_LIVE_READ_OK");
    let target = workspace.path().join("read-target.txt");
    let prompt = format!(
        "Use read_files to read '{}'. Do not answer before using the tool. After it succeeds, reply exactly CLINE_LIVE_OK.",
        target.display()
    );
    let output = live_command(
        workspace.path(),
        [
            "run",
            "cline",
            "--model",
            "qwen3.6",
            "--",
            "--json",
            "--timeout",
            "120",
            &prompt,
        ],
    )
    .run()
    .await
    .expect("Cline live conformance should complete before the timeout");
    assert_success(&output);
    assert!(output.stdout.contains("read_files"), "{}", output.stdout);
    assert!(output.stdout.contains("CLINE_LIVE_OK"), "{}", output.stdout);
    assert!(
        !output.stdout.contains("\"is_error\":true"),
        "{}",
        output.stdout
    );
}

#[tokio::test]
#[ignore = "requires Qwen Code, network access, and NAN_API_KEY"]
async fn qwen_code_completes_a_real_read_tool_round_trip() {
    let workspace = live_workspace("QWEN_LIVE_READ_OK");
    let target = workspace.path().join("read-target.txt");
    let prompt = format!(
        "Use read_file to read '{}'. Do not answer before using the tool. After it succeeds, reply exactly QWEN_LIVE_OK.",
        target.display()
    );
    let output = live_command(
        workspace.path(),
        [
            "run",
            "qwen-code",
            "--model",
            "qwen3.6",
            "--",
            "--safe-mode",
            "--prompt",
            &prompt,
            "--output-format",
            "stream-json",
        ],
    )
    .run()
    .await
    .expect("Qwen Code live conformance should complete before the timeout");
    assert_success(&output);
    assert!(
        output.stdout.contains("\"name\":\"read_file\""),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("QWEN_LIVE_OK"), "{}", output.stdout);
    assert!(
        output.stdout.contains("\"is_error\":false"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("\"is_error\":true"),
        "{}",
        output.stdout
    );
}

fn live_workspace(content: &str) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("live workspace should exist");
    std::fs::create_dir_all(workspace.path().join(".live-home"))
        .expect("isolated live home should exist");
    std::fs::write(workspace.path().join("read-target.txt"), content)
        .expect("live read fixture should exist");
    workspace
}

fn live_command<const N: usize>(workspace: &Path, arguments: [&str; N]) -> TerminalCommand {
    assert!(
        std::env::var_os("NAN_API_KEY").is_some(),
        "NAN_API_KEY must be set for live conformance"
    );
    TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace)
        .args(arguments.into_iter().map(OsString::from))
        .env("HOME", workspace.join(".live-home"))
        .timeout(Duration::from_mins(4))
}

fn assert_success(output: &TerminalOutput) {
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(
        !output.stdout.contains("NH-BRIDGE-"),
        "{}",
        output.diagnostic()
    );
}
