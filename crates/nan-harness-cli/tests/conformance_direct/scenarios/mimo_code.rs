use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned MiMo Code executable"]
async fn mimo_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_fixture(workspace.path(), "read-target.txt", "MIMO_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "MIMO_BEFORE\n");
    let config_path = workspace.path().join(".mimocode/mimocode.json");
    let user_config = r#"{"model":"other/old","small_model":"other/old","model_groups":{"lite":"other/old"},"provider":{"other":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:1/v1","apiKey":"synthetic-unused"},"models":{"old":{}}}}}"#;
    write_fixture(workspace.path(), ".mimocode/mimocode.json", user_config);
    let path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read",
            json!({"file_path": format!("{path}/read-target.txt")}),
        ),
        call(
            "write",
            json!({"file_path": format!("{path}/write-output.txt"), "content": "MIMO_WRITE_OK\n"}),
        ),
        call(
            "read",
            json!({"file_path": format!("{path}/edit-target.txt")}),
        ),
        call(
            "edit",
            json!({"file_path": format!("{path}/edit-target.txt"), "old_string": "MIMO_BEFORE", "new_string": "MIMO_AFTER"}),
        ),
        call(
            "bash",
            json!({"command": "printf MIMO_BASH_OK > bash-output.txt", "description": "Write a deterministic conformance marker file"}),
        ),
        call("glob", json!({"pattern": "*.txt", "path": path})),
        call("grep", json!({"pattern": "MIMO_READ_OK", "path": path})),
        call(
            "actor",
            json!({"operation": {"action": "run", "subagent_type": "general", "description": "Verify auxiliary routing", "prompt": "Reply with the conformance helper marker without tools.", "model": "lite", "context": "none", "timeout_ms": 30000}}),
        ),
    ];
    let requests = run_round_trip(
        "mimo-code",
        [
            "run",
            "--pure",
            "--format",
            "json",
            "--dangerously-skip-permissions",
            "Complete the deterministic native tool sequence.",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_MIMO_TOOLS_OK",
    )
    .await;
    assert!(requests.iter().all(|request| request["model"] == "qwen3.6"));
    assert!(
        requests
            .iter()
            .filter_map(nan_harness_test_support::conformance::tool_names)
            .any(|tools| tools.contains("bash") && !tools.contains("actor")),
        "the general subagent should request its own tool catalog"
    );
    assert_file(workspace.path(), "write-output.txt", "MIMO_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "MIMO_AFTER");
    assert_file(workspace.path(), "bash-output.txt", "MIMO_BASH_OK");
    let mut actual: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).expect("user config"))
            .expect("config JSON");
    // MiMo itself adds its schema on first load; every user-owned setting must survive.
    let schema = actual.as_object_mut().expect("object").remove("$schema");
    assert_eq!(
        schema,
        Some(json!("https://mimo.xiaomi.com/mimocode/config.json"))
    );
    assert_eq!(
        actual,
        serde_json::from_str::<serde_json::Value>(user_config).expect("original config")
    );
}

#[tokio::test]
#[ignore = "requires the pinned MiMo Code executable"]
async fn mimo_native_inventory_reaches_nan() {
    let tools = inventory(
        "mimo-code",
        [
            "run",
            "--pure",
            "--format",
            "json",
            "--dangerously-skip-permissions",
            "Reply with the inventory marker.",
        ],
        &[],
    )
    .await;
    let manifest = nan_harness_test_support::conformance::harness_registration(
        nan_harness_core::HarnessKind::MimoCode,
    )
    .expect("MiMo registration")
    .manifest()
    .expect("MiMo manifest");
    let expected = manifest
        .inventory
        .into_iter()
        .chain(["nan-search_web_search".to_owned()])
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(tools, expected);
    assert!(tools.contains("read"), "MiMo inventory: {tools:?}");
    assert!(tools.contains("bash"));
}
