#![cfg(unix)]

use nan_harness_test_support::conformance::assert_success;
use nan_harness_test_support::terminal::TerminalCommand;
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
            "qwen",
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

#[tokio::test]
#[ignore = "requires Kimi Code, network access, and NAN_API_KEY"]
async fn kimi_code_completes_a_real_read_tool_round_trip() {
    let workspace = live_workspace("KIMI_LIVE_READ_OK");
    let target = workspace.path().join("read-target.txt");
    let prompt = format!(
        "Use Read to read '{}'. Do not answer before using the tool. After it succeeds, reply exactly KIMI_LIVE_OK.",
        target.display()
    );
    let output = live_command(
        workspace.path(),
        [
            "kimi",
            "--model",
            "qwen3.6",
            "--",
            "--prompt",
            &prompt,
            "--output-format",
            "stream-json",
        ],
    )
    .run()
    .await
    .expect("Kimi Code live conformance should complete before the timeout");
    assert_success(&output);
    assert!(output.stdout.contains("Read"), "{}", output.stdout);
    assert!(output.stdout.contains("KIMI_LIVE_OK"), "{}", output.stdout);
    assert!(
        !output.stdout.contains("\"isError\":true"),
        "{}",
        output.stdout
    );
}

#[tokio::test]
#[ignore = "requires Aider, network access, and NAN_API_KEY"]
async fn aider_completes_a_real_edit_round_trip() {
    let workspace = live_workspace("AIDER_LIVE_EDIT_BEFORE");
    let target = workspace.path().join("read-target.txt");
    let output = live_command(
        workspace.path(),
        [
            "aider",
            "--model",
            "qwen3.6",
            "--",
            "--message",
            "Replace the entire file content with exactly AIDER_LIVE_EDIT_OK.",
            "--yes-always",
            "--no-auto-commits",
            "--no-git",
            "--edit-format",
            "whole",
            "--no-show-model-warnings",
            "--no-check-update",
            "--map-tokens",
            "0",
            "read-target.txt",
        ],
    )
    .run()
    .await
    .expect("Aider live conformance should complete before the timeout");
    assert_success(&output);
    assert_eq!(
        std::fs::read_to_string(target)
            .expect("Aider should leave the edited fixture readable")
            .trim(),
        "AIDER_LIVE_EDIT_OK"
    );
}

#[tokio::test]
#[ignore = "requires Goose, network access, and NAN_API_KEY"]
async fn goose_completes_a_real_tree_tool_round_trip() {
    let workspace = live_workspace("GOOSE_LIVE_TREE_OK");
    let workspace_path = workspace.path().to_string_lossy();
    let prompt = format!(
        "Use the tree tool on '{workspace_path}' with depth 2. Do not answer before the tool succeeds. Then reply exactly GOOSE_LIVE_OK."
    );
    let output = live_command(
        workspace.path(),
        [
            "goose",
            "--model",
            "qwen3.6",
            "--",
            "run",
            "--no-profile",
            "--no-session",
            "--with-builtin",
            "developer",
            "--output-format",
            "json",
            "--text",
            &prompt,
        ],
    )
    .run()
    .await
    .expect("Goose live conformance should complete before the timeout");
    assert_success(&output);
    assert!(output.stdout.contains("tree"), "{}", output.stdout);
    assert!(output.stdout.contains("GOOSE_LIVE_OK"), "{}", output.stdout);
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
