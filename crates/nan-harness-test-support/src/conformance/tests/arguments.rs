use crate::conformance::{
    RunKind, ScriptedToolCall, TEST_CREDENTIAL, conformance_command, headless_arguments,
};
use nan_harness_core::HarnessKind;
use serde_json::json;
use std::ffi::OsString;

fn exact_args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn tool_run_kind() -> RunKind {
    RunKind::Tool(ScriptedToolCall {
        name: "read_file".to_owned(),
        input: json!({"path": "fixture.txt"}),
        result_expected: true,
    })
}

fn external_run_kind() -> RunKind {
    RunKind::External {
        tool: "DesignSync".to_owned(),
        arguments: vec!["--fixture".to_owned(), "http://fixture".to_owned()],
        enabled_tools: vec!["DesignSync".to_owned(), "read_file".to_owned()],
    }
}

#[test]
fn claude_headless_arguments_are_exact_for_every_run_kind() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let cases = [
        (
            RunKind::Inventory,
            exact_args(&[
                "-p",
                "Reply exactly INVENTORY without using tools.",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--no-session-persistence",
                "--max-turns",
                "12",
            ]),
        ),
        (
            tool_run_kind(),
            exact_args(&[
                "-p",
                "Use the read_file tool exactly once, wait for its result, then reply exactly TOOL.",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--no-session-persistence",
                "--max-turns",
                "12",
                "--tools",
                "read_file",
                "--allowedTools",
                "read_file",
            ]),
        ),
        (
            RunKind::Sentinel,
            exact_args(&[
                "-p",
                "Reply exactly SENTINEL without using tools.",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--no-session-persistence",
                "--max-turns",
                "12",
            ]),
        ),
        (
            external_run_kind(),
            exact_args(&[
                "-p",
                "Run the deterministic DesignSync authorization scenario, report its controlled prerequisite, then reply exactly EXTERNAL.",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--no-session-persistence",
                "--max-turns",
                "12",
                "--tools",
                "DesignSync,read_file",
                "--allowedTools",
                "DesignSync,read_file",
                "--fixture",
                "http://fixture",
            ]),
        ),
    ];
    for (run_kind, expected) in cases {
        let marker = match &run_kind {
            RunKind::Inventory => "INVENTORY",
            RunKind::Tool(_) => "TOOL",
            RunKind::Sentinel => "SENTINEL",
            RunKind::External { .. } => "EXTERNAL",
        };
        assert_eq!(
            headless_arguments(HarnessKind::ClaudeCode, &run_kind, marker, workspace.path()),
            expected
        );
    }
}

#[test]
fn qwen_headless_arguments_are_exact_for_every_run_kind() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let cases = [
        (
            RunKind::Inventory,
            exact_args(&[
                "--safe-mode",
                "--prompt",
                "Reply exactly INVENTORY without using tools.",
                "--output-format",
                "json",
            ]),
        ),
        (
            tool_run_kind(),
            exact_args(&[
                "--safe-mode",
                "--prompt",
                "Use the read_file tool exactly once, wait for its result, then reply exactly TOOL.",
                "--output-format",
                "json",
                "--allowed-tools",
                "read_file",
            ]),
        ),
        (
            RunKind::Sentinel,
            exact_args(&[
                "--safe-mode",
                "--prompt",
                "Reply exactly SENTINEL without using tools.",
                "--output-format",
                "json",
            ]),
        ),
        (
            external_run_kind(),
            exact_args(&[
                "--safe-mode",
                "--prompt",
                "Run the deterministic DesignSync authorization scenario, report its controlled prerequisite, then reply exactly EXTERNAL.",
                "--output-format",
                "json",
            ]),
        ),
    ];
    for (run_kind, expected) in cases {
        let marker = match &run_kind {
            RunKind::Inventory => "INVENTORY",
            RunKind::Tool(_) => "TOOL",
            RunKind::Sentinel => "SENTINEL",
            RunKind::External { .. } => "EXTERNAL",
        };
        assert_eq!(
            headless_arguments(HarnessKind::QwenCode, &run_kind, marker, workspace.path()),
            expected
        );
    }
}

#[test]
fn prime_headless_arguments_are_exact_for_every_run_kind() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let socket = workspace.path().join("home/prime-agent.sock");
    let cases = [
        (
            RunKind::Inventory,
            exact_args(&[
                "--mode",
                "json",
                "--print",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--tools",
                "ipython",
                "Reply exactly INVENTORY without using tools.",
                "--daemon-socket",
            ]),
        ),
        (
            tool_run_kind(),
            exact_args(&[
                "--mode",
                "json",
                "--print",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--tools",
                "ipython",
                "Use the read_file tool exactly once, wait for its result, then reply exactly TOOL.",
                "--daemon-socket",
            ]),
        ),
        (
            RunKind::Sentinel,
            exact_args(&[
                "--mode",
                "json",
                "--print",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--tools",
                "ipython",
                "Reply exactly SENTINEL without using tools.",
                "--daemon-socket",
            ]),
        ),
        (
            external_run_kind(),
            exact_args(&[
                "--mode",
                "json",
                "--print",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--tools",
                "ipython",
                "Run the deterministic DesignSync authorization scenario, report its controlled prerequisite, then reply exactly EXTERNAL.",
                "--daemon-socket",
            ]),
        ),
    ];
    for (run_kind, mut expected) in cases {
        let marker = match &run_kind {
            RunKind::Inventory => "INVENTORY",
            RunKind::Tool(_) => "TOOL",
            RunKind::Sentinel => "SENTINEL",
            RunKind::External { .. } => "EXTERNAL",
        };
        expected.push(socket.clone().into_os_string());
        assert_eq!(
            headless_arguments(HarnessKind::PrimeAgent, &run_kind, marker, workspace.path()),
            expected
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn conformance_command_replaces_a_parent_api_key() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir().expect("workspace should exist");
    let script = workspace.path().join("assert-environment.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$NAN_API_KEY\" = \"{TEST_CREDENTIAL}\" ]\n[ \"$NAN_NO_UPDATE_CHECK\" = 1 ]\n"
        ),
    )
    .expect("environment assertion script should be written");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
        .expect("environment assertion script should be executable");
    let output = conformance_command(
        script,
        HarnessKind::Fx,
        workspace.path(),
        "http://127.0.0.1:1/v1",
    )
    .run()
    .await
    .expect("environment assertion command should run");
    assert!(output.status.success(), "{}", output.diagnostic());
}
