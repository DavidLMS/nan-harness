#![cfg(unix)]

use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::TerminalCommand;
use std::ffi::OsString;
use std::time::Duration;

const INVENTORY_MARKER: &str = "NAN_HARNESS_CODEX_INVENTORY_OK";

#[tokio::test]
#[ignore = "requires the pinned Codex executable"]
async fn codex_native_inventory_crosses_the_responses_bridge() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let codex_home = tempfile::tempdir().expect("Codex home should exist");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let output = TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args(vec![
            OsString::from("codex"),
            OsString::from("--provider-base-url"),
            OsString::from(provider.base_url()),
            OsString::from("--"),
            OsString::from("exec"),
            OsString::from("--skip-git-repo-check"),
            OsString::from("--ephemeral"),
            OsString::from("--json"),
            OsString::from(format!(
                "Reply exactly {INVENTORY_MARKER} without using tools."
            )),
        ])
        .env("NAN_API_KEY", "nan_test_key")
        .env("CODEX_HOME", codex_home.path())
        .timeout(Duration::from_secs(90))
        .run()
        .await
        .expect("NaN Harness should complete before the timeout");

    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(output.stdout.contains(INVENTORY_MARKER));
    assert!(!output.stdout.contains("NH-BRIDGE-"));
    let requests = provider.chat_requests();
    let tools = requests
        .first()
        .and_then(|request| request.get("tools"))
        .and_then(serde_json::Value::as_array)
        .expect("Codex should advertise tools");
    let tool_names = tools
        .iter()
        .filter_map(|entry| entry.pointer("/function/name"))
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    for tool in ["exec_command", "write_stdin", "apply_patch", "update_plan"] {
        assert!(
            tools.iter().any(|entry| {
                entry
                    .pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(tool)
            }),
            "Codex tool '{tool}' should cross the bridge; received {tool_names:?}"
        );
    }
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}
