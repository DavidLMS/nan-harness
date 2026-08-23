#![cfg(unix)]

use nan_harness_core::HarnessKind;
use nan_harness_test_support::conformance::conformance_command;
use nan_harness_test_support::scripted_provider::{
    ProviderScenario, ScriptedProvider, ScriptedToolCall,
};
use nan_harness_test_support::terminal::TerminalCommand;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

const INVENTORY_MARKER: &str = "NAN_HARNESS_FX_INVENTORY_OK";

#[tokio::test]
#[ignore = "requires the pinned fx executable"]
async fn fx_native_inventory_crosses_the_gateway_bridge() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let home = workspace.path().join("home");
    std::fs::create_dir_all(&home).expect("isolated home should exist");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let output = conformance_command(
        env!("CARGO_BIN_EXE_nan-harness"),
        HarnessKind::Fx,
        workspace.path(),
        provider.base_url(),
    )
    .args([
        "ask",
        "--yolo",
        "--no-save",
        "--no-color",
        &format!("Reply exactly {INVENTORY_MARKER} without using tools."),
    ])
    .env("HOME", &home)
    .run()
    .await
    .expect("nan-harness should complete before the timeout");

    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(output.stdout.contains(INVENTORY_MARKER));
    assert!(!output.stdout.contains("NH-BRIDGE-"));
    let tools = provider
        .chat_requests()
        .iter()
        .find_map(tool_names)
        .expect("fx should advertise tools");
    assert_eq!(
        tools,
        BTreeSet::from([
            "ask_user_question".to_owned(),
            "copy_file".to_owned(),
            "create_folder".to_owned(),
            "delete_file".to_owned(),
            "edit_file".to_owned(),
            "file_info".to_owned(),
            "glob_files".to_owned(),
            "grep_files".to_owned(),
            "install_skill".to_owned(),
            "list_files".to_owned(),
            "mcp_features".to_owned(),
            "mcp_search_tools".to_owned(),
            "mcp_select_tool".to_owned(),
            "memory".to_owned(),
            "open_file".to_owned(),
            "perplexity_search".to_owned(),
            "read_file".to_owned(),
            "read_tool_result".to_owned(),
            "rename_file".to_owned(),
            "semantic_search".to_owned(),
            "skill".to_owned(),
            "subagent".to_owned(),
            "terminal".to_owned(),
            "vision".to_owned(),
            "web_fetch".to_owned(),
            "write_file".to_owned(),
        ])
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned fx executable"]
async fn fx_local_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let home = workspace.path().join("home");
    std::fs::create_dir_all(&home).expect("isolated home should exist");
    write_fixture(workspace.path(), "read-target.txt", "FX_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "FX_EDIT_BEFORE\n");
    write_fixture(workspace.path(), "copy-source.txt", "FX_COPY_OK\n");
    write_fixture(workspace.path(), "rename-source.txt", "FX_RENAME_OK\n");
    write_fixture(workspace.path(), "delete-target.txt", "FX_DELETE_ME\n");
    write_fixture(
        workspace.path(),
        ".agents/skills/conformance/SKILL.md",
        "---\nname: conformance\ndescription: Return the conformance marker.\n---\n\nReturn FX_SKILL_OK.\n",
    );
    let calls = vec![
        call("read_file", json!({"path": "read-target.txt"})),
        call("file_info", json!({"path": "read-target.txt"})),
        call("list_files", json!({"path": "."})),
        call("glob_files", json!({"pattern": "*.txt", "path": "."})),
        call("grep_files", json!({"pattern": "FX_READ_OK", "path": "."})),
        call(
            "semantic_search",
            json!({"query": "read marker", "path": "."}),
        ),
        call("create_folder", json!({"path": "created-directory"})),
        call(
            "write_file",
            json!({"path": "write-output.txt", "content": "FX_WRITE_OK\n"}),
        ),
        call(
            "copy_file",
            json!({"source": "copy-source.txt", "destination": "copy-output.txt"}),
        ),
        call(
            "rename_file",
            json!({"old_path": "rename-source.txt", "new_path": "rename-output.txt"}),
        ),
        call(
            "edit_file",
            json!({
                "path": "edit-target.txt",
                "old_string": "FX_EDIT_BEFORE",
                "new_string": "FX_EDIT_AFTER"
            }),
        ),
        call(
            "terminal",
            json!({"action": "exec", "command": "printf FX_TERMINAL_OK"}),
        ),
        call("skill", json!({"name": "conformance"})),
        call("memory", json!({"action": "list"})),
        call("delete_file", json!({"path": "delete-target.txt"})),
    ];
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        calls.iter().cloned(),
        "NAN_HARNESS_FX_TOOLS_OK",
    ))
    .await
    .expect("scripted provider should start");
    let output = fx_command(
        workspace.path(),
        &home,
        provider.base_url(),
        "Complete the deterministic native tool conformance sequence.",
    )
    .run()
    .await
    .expect("nan-harness should complete before the timeout");

    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(output.stdout.contains("NAN_HARNESS_FX_TOOLS_OK"));
    assert!(!output.stdout.contains("NH-BRIDGE-"));
    let requests = provider.chat_requests();
    for (index, tool_call) in calls.iter().enumerate() {
        let identifier = format!("call_nan_harness_conformance_{index}");
        let result = tool_result(&requests, &identifier)
            .unwrap_or_else(|| panic!("fx did not return a result for tool {}", tool_call.name));
        assert!(
            !tool_result_failed(&result),
            "fx tool {} returned an error: {result}",
            tool_call.name
        );
    }
    assert_file(workspace.path(), "write-output.txt", "FX_WRITE_OK");
    assert_file(workspace.path(), "copy-output.txt", "FX_COPY_OK");
    assert_file(workspace.path(), "rename-output.txt", "FX_RENAME_OK");
    assert_file(workspace.path(), "edit-target.txt", "FX_EDIT_AFTER");
    assert!(!workspace.path().join("delete-target.txt").exists());
    assert!(workspace.path().join("created-directory").is_dir());
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

fn fx_command(
    workspace: &Path,
    home: &Path,
    provider_base_url: &str,
    prompt: &str,
) -> TerminalCommand {
    conformance_command(
        env!("CARGO_BIN_EXE_nan-harness"),
        HarnessKind::Fx,
        workspace,
        provider_base_url,
    )
    .args([
        OsString::from("ask"),
        OsString::from("--yolo"),
        OsString::from("--no-save"),
        OsString::from("--no-color"),
        OsString::from(prompt),
    ])
    .env("HOME", home)
    .timeout(Duration::from_secs(90))
}

fn call(name: &str, input: Value) -> ScriptedToolCall {
    ScriptedToolCall {
        name: name.to_owned(),
        input,
        result_expected: true,
    }
}

fn tool_result(requests: &[Value], identifier: &str) -> Option<String> {
    requests.iter().find_map(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().find_map(|message| {
                    let matches = message.get("role").and_then(Value::as_str) == Some("tool")
                        && message
                            .get("tool_call_id")
                            .and_then(Value::as_str)
                            .is_some_and(|actual| {
                                actual
                                    .chars()
                                    .filter(char::is_ascii_alphanumeric)
                                    .eq(identifier.chars().filter(char::is_ascii_alphanumeric))
                            });
                    matches.then(|| {
                        message
                            .get("content")
                            .map_or_else(|| message.to_string(), ToString::to_string)
                    })
                })
            })
    })
}

fn tool_result_failed(result: &str) -> bool {
    if result
        .trim_matches('"')
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("error")
    {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return false;
    };
    value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

fn write_fixture(workspace: &Path, relative_path: &str, content: &str) {
    let path = workspace.join(relative_path);
    std::fs::create_dir_all(path.parent().expect("fixture should have a parent"))
        .expect("fixture directory should exist");
    std::fs::write(path, content).expect("fixture should be written");
}

fn assert_file(workspace: &Path, relative_path: &str, expected: &str) {
    let content = std::fs::read_to_string(workspace.join(relative_path))
        .expect("expected conformance file should exist");
    assert!(content.contains(expected), "file content was {content:?}");
}

fn tool_names(request: &Value) -> Option<BTreeSet<String>> {
    request.get("tools")?.as_array().map(|tools| {
        tools
            .iter()
            .filter_map(|tool| tool.pointer("/function/name").or_else(|| tool.get("name")))
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    })
}
