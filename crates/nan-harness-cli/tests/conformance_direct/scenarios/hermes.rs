use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned Hermes executable"]
async fn hermes_local_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "HERMES_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "HERMES_EDIT_BEFORE\n");
    let calls = hermes_local_tool_calls(workspace.path());
    run_round_trip(
        "hermes",
        [
            "chat",
            "--query",
            "Complete the deterministic native tool conformance sequence.",
            "--quiet",
            "--yolo",
            "--safe-mode",
            "--source",
            "tool",
            "--max-turns",
            "30",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_HERMES_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "HERMES_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "HERMES_EDIT_AFTER");
    assert_file(
        workspace.path(),
        "terminal-output.txt",
        "HERMES_TERMINAL_OK",
    );
}

fn hermes_local_tool_calls(workspace: &Path) -> Vec<ScriptedToolCall> {
    let workspace_path = workspace.to_string_lossy();
    let skill = concat!(
        "---\n",
        "name: conformance\n",
        "description: Verify Hermes skill tools\n",
        "---\n\n",
        "HERMES_SKILL_OK\n"
    );
    vec![
        call(
            "read_file",
            json!({"path": format!("{workspace_path}/read-target.txt")}),
        ),
        call(
            "write_file",
            json!({
                "path": format!("{workspace_path}/write-output.txt"),
                "content": "HERMES_WRITE_OK\n"
            }),
        ),
        call(
            "patch",
            json!({
                "mode": "replace",
                "path": format!("{workspace_path}/edit-target.txt"),
                "old_string": "HERMES_EDIT_BEFORE",
                "new_string": "HERMES_EDIT_AFTER"
            }),
        ),
        call(
            "search_files",
            json!({
                "pattern": "HERMES_READ_OK",
                "path": workspace_path,
                "target": "content"
            }),
        ),
        call(
            "terminal",
            json!({
                "command": "printf HERMES_TERMINAL_OK > terminal-output.txt",
                "workdir": workspace_path
            }),
        ),
        call("process", json!({"action": "list"})),
        call(
            "execute_code",
            json!({"code": "print('HERMES_EXECUTE_CODE_OK')"}),
        ),
        call(
            "todo",
            json!({
                "todos": [{
                    "id": "conformance",
                    "content": "Verify Hermes tools",
                    "status": "completed"
                }]
            }),
        ),
        call(
            "memory",
            json!({
                "target": "memory",
                "action": "add",
                "content": "HERMES_MEMORY_OK"
            }),
        ),
        call(
            "skill_manage",
            json!({"action": "create", "name": "conformance", "content": skill}),
        ),
        call("skills_list", json!({})),
        call("skill_view", json!({"name": "conformance"})),
        call(
            "session_search",
            json!({"query": "native tool conformance"}),
        ),
        call("cronjob", json!({"action": "list"})),
        call(
            "delegate_task",
            json!({
                "goal": "Reply exactly HERMES_SUBAGENT_OK without using tools.",
                "context": "This is a deterministic nan-harness conformance check."
            }),
        ),
    ]
}

#[tokio::test]
#[ignore = "requires the pinned Hermes executable and optional tool runtimes"]
async fn hermes_environment_bound_tools_return_controlled_results() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let harness_arguments = [
        "chat",
        "--query",
        "Complete the environment-bound native tool conformance check.",
        "--quiet",
        "--yolo",
        "--safe-mode",
        "--source",
        "tool",
        "--max-turns",
        "4",
    ];
    let inventory = inventory(
        "hermes",
        [
            "chat",
            "--query",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
            "--quiet",
            "--yolo",
            "--safe-mode",
            "--max-turns",
            "2",
        ],
        HERMES_OPTIONAL_CREDENTIALS_CLEARED,
    )
    .await;
    assert_hermes_inventory(&inventory);

    let mut calls = vec![call(
        "text_to_speech",
        json!({"text": "Hermes conformance", "provider": "edge"}),
    )];
    if inventory.contains("browser_exec") {
        calls.push(call(
            "browser_exec",
            json!({
                "code": "# Verify browser runtime\nprint('HERMES_BROWSER_OK')",
                "timeout_s": 5
            }),
        ));
    } else {
        calls.extend([
            call(
                "browser_navigate",
                json!({"url": "http://127.0.0.1:9/nan-harness-conformance"}),
            ),
            call("browser_snapshot", json!({})),
            call("browser_click", json!({"ref": "@missing"})),
            call(
                "browser_type",
                json!({"ref": "@missing", "text": "Hermes conformance"}),
            ),
            call("browser_scroll", json!({"direction": "down"})),
            call("browser_back", json!({})),
            call("browser_press", json!({"key": "Escape"})),
            call("browser_get_images", json!({})),
            call("browser_console", json!({})),
        ]);
    }
    if inventory.contains("computer_use") {
        calls.push(call("computer_use", json!({"action": "list_apps"})));
    }

    for tool_call in calls {
        run_controlled_tool(
            "hermes",
            &harness_arguments,
            HERMES_OPTIONAL_CREDENTIALS_CLEARED,
            &workspace,
            tool_call,
        )
        .await;
    }
}
