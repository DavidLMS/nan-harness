use super::support::*;

const HEADLESS_ARGUMENTS: [&str; 6] = [
    "--trust",
    "--auto-approve",
    "--output",
    "text",
    "--prompt",
    "Complete the deterministic native tool sequence.",
];

#[tokio::test]
#[ignore = "requires the pinned Mistral Vibe executable"]
async fn vibe_published_conformance_reports_a_safe_success() {
    use nan_harness_test_support::conformance::PublishedConformanceRunner;
    let report = PublishedConformanceRunner::new(
        env!("CARGO_BIN_EXE_nan-harness"),
        nan_harness_core::HarnessKind::MistralVibe,
    )
    .run()
    .await
    .expect("published conformance should produce a safe report");
    assert!(report.is_success(), "{report:?}");
}

#[tokio::test]
#[ignore = "requires the pinned Mistral Vibe executable"]
async fn vibe_native_tools_complete_round_trips_and_keep_user_configuration() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_fixture(workspace.path(), "read-target.txt", "VIBE_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "VIBE_BEFORE\n");
    let user_config = "textual_theme = \"dark\"\n\n[[mcp_servers]]\nname = \"user-docs\"\ntransport = \"stdio\"\ncommand = \"user-docs-server\"\n";
    write_fixture(
        workspace.path(),
        ".conformance-home/.vibe/config.toml",
        user_config,
    );
    let path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read_file",
            json!({"path": format!("{path}/read-target.txt")}),
        ),
        call(
            "write_file",
            json!({"path": format!("{path}/write-output.txt"), "content": "VIBE_WRITE_OK\n"}),
        ),
        call(
            "read_file",
            json!({"path": format!("{path}/edit-target.txt")}),
        ),
        call(
            "edit",
            json!({"file_path": format!("{path}/edit-target.txt"), "old_string": "VIBE_BEFORE", "new_string": "VIBE_AFTER"}),
        ),
        call(
            "bash",
            json!({"command": "printf VIBE_BASH_OK > bash-output.txt"}),
        ),
        call(
            "todo",
            json!({"action": "write", "todos": [{"id": "1", "content": "Verify Vibe tools", "status": "completed"}]}),
        ),
    ];
    let requests = run_round_trip(
        "mistral-vibe",
        HEADLESS_ARGUMENTS,
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_VIBE_TOOLS_OK",
    )
    .await;

    // Vibe reports schema rejections as an ordinary tool result, so inspect them.
    for index in 0..6 {
        let result = nan_harness_test_support::conformance::tool_result(
            &requests,
            &format!("call_nan_harness_conformance_{index}"),
        )
        .expect("every scripted call should return a result");
        assert!(!result.contains("InvalidToolCall"), "call {index}");
    }
    assert!(requests.iter().all(|request| request["model"] == "qwen3.6"));
    assert_file(workspace.path(), "write-output.txt", "VIBE_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "VIBE_AFTER");
    assert_file(workspace.path(), "bash-output.txt", "VIBE_BASH_OK");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join(".conformance-home/.vibe/config.toml"))
            .expect("user configuration should remain readable"),
        user_config
    );
}

#[tokio::test]
#[ignore = "requires the pinned Mistral Vibe executable"]
async fn vibe_native_inventory_reaches_nan() {
    let tools = inventory(
        "mistral-vibe",
        [
            "--trust",
            "--auto-approve",
            "--output",
            "text",
            "--prompt",
            "Reply with the inventory marker.",
        ],
        &[],
    )
    .await;
    let manifest = nan_harness_test_support::conformance::harness_registration(
        nan_harness_core::HarnessKind::MistralVibe,
    )
    .expect("Vibe registration")
    .manifest()
    .expect("Vibe manifest");
    assert!(
        manifest.inventory.iter().all(|name| tools.contains(name)),
        "Vibe inventory: {tools:?}"
    );
    // MCP functions such as the NaN search server are reached through
    // `search_tool_functions`, so the advertised inventory is exactly the manifest.
    assert_eq!(
        tools,
        manifest.tool_names(),
        "Vibe advertised an unexpected inventory"
    );
}
