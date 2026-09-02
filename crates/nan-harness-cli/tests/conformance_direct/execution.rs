use super::fixtures::INVENTORY_MARKER;
use nan_harness_core::HarnessKind;
use nan_harness_test_support::assertions::assert_tool_results;
use nan_harness_test_support::conformance::{
    assert_success, conformance_command, tool_names, tool_result, tool_result_failed,
};
use nan_harness_test_support::scripted_provider::{
    ProviderScenario, ScriptedProvider, ScriptedToolCall,
};
use nan_harness_test_support::terminal::TerminalCommand;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub(super) async fn inventory<const N: usize>(
    harness: &str,
    harness_arguments: [&str; N],
    environment: &[(&str, &str)],
) -> BTreeSet<String> {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from(harness_command_name(harness)),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.into_iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "nan-harness should complete before the timeout: {error}\nprovider progress: {:#?}",
                request_tool_progress(&provider.chat_requests())
            )
        });
    assert_success(&output);
    assert!(
        output.stdout.contains(INVENTORY_MARKER),
        "{}",
        output.diagnostic()
    );
    let requests = provider.chat_requests();
    let tools = requests
        .iter()
        .find_map(tool_names)
        .expect("the harness should advertise at least one tool");
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
    tools
}

pub(super) async fn run_round_trip<const N: usize>(
    harness: &str,
    harness_arguments: [&str; N],
    environment: &[(&str, &str)],
    workspace: &tempfile::TempDir,
    calls: Vec<ScriptedToolCall>,
    allowed_errors: &[&str],
    final_marker: &str,
) {
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        calls.iter().cloned(),
        final_marker,
    ))
    .await
    .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from(harness_command_name(harness)),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.into_iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "nan-harness should complete before the timeout: {error}\nprovider progress: {:#?}",
                request_tool_progress(&provider.chat_requests())
            )
        });
    assert!(output.status.success(), "{}", output.diagnostic());
    let requests = provider.chat_requests();
    assert!(
        provider.completed(),
        "{harness} should receive the final response"
    );
    assert_tool_results(&requests, &calls, allowed_errors).unwrap_or_else(|error| {
        panic!(
            "{harness} scripted results failed: {error}\n{}",
            output.diagnostic()
        )
    });
    assert!(
        output.stdout.contains(final_marker),
        "{}",
        output.diagnostic()
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

pub(super) async fn run_controlled_tool(
    harness: &str,
    harness_arguments: &[&str],
    environment: &[(&str, &str)],
    workspace: &tempfile::TempDir,
    tool_call: ScriptedToolCall,
) {
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        tool_call.name.clone(),
        tool_call.input,
        "NAN_HARNESS_CONTROLLED_TOOL_OK",
    ))
    .await
    .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from(harness_command_name(harness)),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .expect("nan-harness should complete before the timeout");
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(
        !output.stderr.contains("Traceback"),
        "{}",
        output.diagnostic()
    );
    let requests = provider.chat_requests();
    let result = tool_result(&requests, "call_nan_harness_conformance_0").unwrap_or_else(|| {
        panic!(
            "{harness} did not return a controlled result for '{}'\n{}",
            tool_call.name,
            output.diagnostic()
        )
    });
    assert!(!result.trim().is_empty(), "{}", output.diagnostic());
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

pub(super) async fn run_openclaw_yield_tool(workspace: &tempfile::TempDir) {
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        "sessions_yield",
        json!({"message": "Wait for deterministic child completion."}),
        "NAN_HARNESS_OPENCLAW_YIELD_OK",
    ))
    .await
    .expect("scripted provider should start");
    let arguments = vec![
        OsString::from("openclaw"),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
        OsString::from("agent"),
        OsString::from("--local"),
        OsString::from("--session-id"),
        OsString::from("nan-harness-yield-tool"),
        OsString::from("--message"),
        OsString::from("Complete the controlled sessions_yield conformance check."),
        OsString::from("--json"),
    ];
    let output = harness_command("openclaw", workspace.path(), arguments, &[])
        .run()
        .await
        .expect("nan-harness should complete before the timeout");
    assert!(output.status.success(), "{}", output.diagnostic());
    let report: Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("OpenClaw should return a JSON report: {error}"));
    assert_eq!(
        report.pointer("/meta/toolSummary/failures"),
        Some(&Value::from(1))
    );
    assert!(
        report
            .pointer("/meta/toolSummary/tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|tool| tool == "sessions_yield")),
        "{}",
        output.diagnostic()
    );
    let requests = provider.chat_requests();
    let result = tool_result(&requests, "call_nan_harness_conformance_0")
        .expect("sessions_yield should return a controlled result");
    assert!(tool_result_failed(&result));
    assert!(
        result.contains("No pending child completion is owned by this turn"),
        "unexpected sessions_yield result: {result}"
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

pub(super) fn harness_command(
    harness: &str,
    workspace: &Path,
    mut arguments: Vec<OsString>,
    environment: &[(&str, &str)],
) -> TerminalCommand {
    if harness == "prime-agent" {
        std::fs::create_dir_all(workspace.join(".conformance-home"))
            .expect("Prime Agent conformance home should exist");
        let separator = arguments
            .iter()
            .position(|argument| argument == "--")
            .expect("nan-harness arguments should include a separator");
        arguments.insert(
            separator + 1,
            prime_daemon_socket(workspace).into_os_string(),
        );
        arguments.insert(separator + 1, OsString::from("--daemon-socket"));
    }
    let provider_base_url = arguments
        .windows(2)
        .find(|pair| pair[0] == "--provider-base-url")
        .and_then(|pair| pair.get(1))
        .expect("nan-harness arguments should include a provider URL")
        .to_string_lossy()
        .into_owned();
    let separator = arguments
        .iter()
        .position(|argument| argument == "--")
        .expect("nan-harness arguments should include a separator");
    let harness_arguments = arguments.split_off(separator + 1);
    let kind = harness
        .parse::<HarnessKind>()
        .expect("conformance harness should be registered");
    let mut command = conformance_command(
        env!("CARGO_BIN_EXE_nan-harness"),
        kind,
        workspace,
        &provider_base_url,
    )
    .args(harness_arguments)
    .timeout(Duration::from_mins(2));
    let isolated_home = workspace.join(".conformance-home");
    match harness {
        "opencode" => {
            command = command
                .env("XDG_CONFIG_HOME", isolated_home.join("config"))
                .env("XDG_DATA_HOME", isolated_home.join("data"))
                .env("XDG_CACHE_HOME", isolated_home.join("cache"));
        }
        "hermes" | "openclaw" | "cline" | "qwen-code" | "kimi-code" | "aider" | "goose" => {
            std::fs::create_dir_all(&isolated_home).expect("conformance home should exist");
            command = command.env("HOME", &isolated_home);
        }
        "pi" | "omp" | "prime-agent" => {
            command = command.env("PI_CODING_AGENT_DIR", isolated_home.join("pi-agent"));
        }
        "deepseek-harness" => command = command.env("DSH_HOME", isolated_home.join("dsh")),
        _ => {}
    }
    if harness == "kimi-code" {
        command = command.timeout(Duration::from_secs(40));
    }
    for (name, value) in environment {
        command = command.env(name, value);
    }
    command
}

fn harness_command_name(harness: &str) -> &str {
    match harness {
        "claude-code" => "claude",
        "prime-agent" => "prime-agent",
        "deepseek-harness" => "dsh",
        "qwen-code" => "qwen",
        "kimi-code" => "kimi",
        _ => harness,
    }
}

struct PrimeDaemonGuard {
    socket: std::path::PathBuf,
}

impl PrimeDaemonGuard {
    fn for_harness(harness: &str, workspace: &Path) -> Option<Self> {
        (harness == "prime-agent").then(|| Self {
            socket: prime_daemon_socket(workspace),
        })
    }
}

impl Drop for PrimeDaemonGuard {
    fn drop(&mut self) {
        let Ok(output) = Command::new("prime-agent")
            .args(["status", "--json"])
            .output()
        else {
            return;
        };
        let Ok(daemons) = serde_json::from_slice::<Value>(&output.stdout) else {
            return;
        };
        let Some(pid) = daemons.as_array().and_then(|entries| {
            entries.iter().find_map(|entry| {
                (entry.get("socketPath").and_then(Value::as_str)
                    == Some(self.socket.to_string_lossy().as_ref()))
                .then(|| entry.get("pid").and_then(Value::as_u64))
                .flatten()
            })
        }) else {
            return;
        };
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn prime_daemon_socket(workspace: &Path) -> std::path::PathBuf {
    workspace.join(".conformance-home/prime-agent.sock")
}

fn request_tool_progress(requests: &[Value]) -> std::collections::BTreeMap<String, String> {
    let mut progress = std::collections::BTreeMap::new();
    for message in requests
        .iter()
        .flat_map(|request| {
            request
                .get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
    {
        let Some(identifier) = message.get("tool_call_id").and_then(Value::as_str) else {
            continue;
        };
        let content = message
            .get("content")
            .map_or_else(String::new, ToString::to_string);
        progress.insert(identifier.to_owned(), content.chars().take(240).collect());
    }
    progress
}
