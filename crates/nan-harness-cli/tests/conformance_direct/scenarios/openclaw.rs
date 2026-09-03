use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned OpenClaw executable"]
#[allow(clippy::too_many_lines)]
async fn openclaw_local_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "OPENCLAW_READ_OK\n");
    write_fixture(
        workspace.path(),
        "edit-target.txt",
        "OPENCLAW_EDIT_BEFORE\n",
    );
    write_fixture(
        workspace.path(),
        ".conformance-home/.openclaw/workspace/MEMORY.md",
        "OPENCLAW_MEMORY_OK\n",
    );
    let workspace_path = workspace.path().to_string_lossy();
    let patch =
        "*** Begin Patch\n*** Add File: patch-output.txt\n+OPENCLAW_PATCH_OK\n*** End Patch";
    let calls = vec![
        call(
            "read",
            json!({"path": format!("{workspace_path}/read-target.txt")}),
        ),
        call(
            "write",
            json!({
                "path": format!("{workspace_path}/write-output.txt"),
                "content": "OPENCLAW_WRITE_OK\n"
            }),
        ),
        call(
            "edit",
            json!({
                "path": format!("{workspace_path}/edit-target.txt"),
                "edits": [{
                    "oldText": "OPENCLAW_EDIT_BEFORE",
                    "newText": "OPENCLAW_EDIT_AFTER"
                }]
            }),
        ),
        call("apply_patch", json!({"input": patch})),
        call(
            "exec",
            json!({
                "command": "printf OPENCLAW_EXEC_OK > exec-output.txt",
                "workdir": workspace_path
            }),
        ),
        call("process", json!({"action": "list"})),
        call("agents_list", json!({})),
        call(
            "create_goal",
            json!({"objective": "Complete the deterministic OpenClaw conformance sequence."}),
        ),
        call("get_goal", json!({})),
        call(
            "update_goal",
            json!({
                "status": "complete",
                "note": "The deterministic goal checks completed."
            }),
        ),
        call("memory_get", json!({"path": "MEMORY.md"})),
        call(
            "memory_search",
            json!({"query": "OPENCLAW_MEMORY_OK", "corpus": "memory"}),
        ),
        call(
            "web_fetch",
            json!({"url": "{{fixture_url}}", "extractMode": "text"}),
        ),
        call("sessions_list", json!({"limit": 10})),
        call(
            "sessions_history",
            json!({"sessionKey": "{{result_id:13}}", "limit": 5, "includeTools": true}),
        ),
        call("session_status", json!({"sessionKey": "current"})),
        call("subagents", json!({"action": "list"})),
        call(
            "sessions_spawn",
            json!({
                "task": "Reply exactly OPENCLAW_SUBAGENT_OK without using tools.",
                "runtime": "subagent",
                "mode": "run",
                "cleanup": "delete"
            }),
        ),
        call("subagents", json!({"action": "list"})),
        call("skill_workshop", json!({"action": "list", "limit": 5})),
        call("node_inference", json!({"action": "discover"})),
        call("nodes", json!({"action": "status"})),
    ];
    run_round_trip(
        "openclaw",
        [
            "agent",
            "--local",
            "--session-id",
            "nan-harness-local-tools",
            "--message",
            "Complete the deterministic native tool conformance sequence.",
            "--json",
        ],
        OPENCLAW_MEDIA_CREDENTIALS_CLEARED,
        &workspace,
        calls,
        &[
            "memory_search",
            "web_fetch",
            "sessions_list",
            "sessions_history",
            "node_inference",
            "nodes",
        ],
        "NAN_HARNESS_OPENCLAW_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "OPENCLAW_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "OPENCLAW_EDIT_AFTER");
    assert_file(
        workspace.path(),
        ".conformance-home/.openclaw/workspace/patch-output.txt",
        "OPENCLAW_PATCH_OK",
    );
    assert_file(workspace.path(), "exec-output.txt", "OPENCLAW_EXEC_OK");
}

#[tokio::test]
#[ignore = "requires the pinned OpenClaw executable"]
async fn openclaw_conditional_media_tools_complete_catalog_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call("image_generate", json!({"action": "list"})),
        call("music_generate", json!({"action": "list"})),
        call("video_generate", json!({"action": "list"})),
    ];
    let mut environment = OPENCLAW_MEDIA_CREDENTIALS_CLEARED.to_vec();
    environment.push(("OPENROUTER_API_KEY", "openrouter_conformance_key"));
    run_round_trip(
        "openclaw",
        [
            "agent",
            "--local",
            "--session-id",
            "nan-harness-media-tools",
            "--message",
            "List every configured native media provider.",
            "--json",
        ],
        &environment,
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_OPENCLAW_MEDIA_TOOLS_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned OpenClaw executable and optional tool runtimes"]
async fn openclaw_environment_bound_tools_return_controlled_results() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call("browser", json!({"action": "status"})),
        call(
            "canvas",
            json!({"action": "snapshot", "node": "missing-conformance-node"}),
        ),
        call(
            "dir_fetch",
            json!({"node": "missing-conformance-node", "path": "/tmp"}),
        ),
        call(
            "dir_list",
            json!({"node": "missing-conformance-node", "path": "/tmp"}),
        ),
        call(
            "file_fetch",
            json!({"node": "missing-conformance-node", "path": "/tmp/missing"}),
        ),
        call(
            "file_write",
            json!({
                "node": "missing-conformance-node",
                "path": "/tmp/nan-harness-conformance.txt",
                "contentBase64": "TkFOX0hBUk5FU1NfT0s="
            }),
        ),
        call("gateway", json!({"action": "config.get"})),
        call(
            "message",
            json!({
                "action": "broadcast",
                "targets": ["missing-conformance-target"],
                "message": "OpenClaw conformance"
            }),
        ),
        call(
            "sessions_send",
            json!({
                "sessionKey": "agent:main:explicit:missing-conformance-session",
                "message": "Reply exactly OPENCLAW_SESSION_SEND_OK.",
                "timeoutSeconds": 1
            }),
        ),
        call("tts", json!({"text": "OpenClaw conformance"})),
        call(
            "web_search",
            json!({"query": "OpenClaw conformance", "count": 1}),
        ),
    ];
    run_round_trip(
        "openclaw",
        [
            "agent",
            "--local",
            "--session-id",
            "nan-harness-environment-tools",
            "--message",
            "Complete the environment-bound native tool conformance sequence.",
            "--json",
        ],
        &[],
        &workspace,
        calls,
        &[
            "browser",
            "canvas",
            "dir_fetch",
            "dir_list",
            "file_fetch",
            "file_write",
            "gateway",
            "message",
            "sessions_send",
            "tts",
            "web_search",
        ],
        "NAN_HARNESS_OPENCLAW_ENVIRONMENT_TOOLS_OK",
    )
    .await;

    run_openclaw_yield_tool(&workspace).await;
}
