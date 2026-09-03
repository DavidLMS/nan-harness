use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned OpenCode executable"]
async fn opencode_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "OPENCODE_READ_OK\n");
    write_fixture(
        workspace.path(),
        "edit-target.txt",
        "OPENCODE_EDIT_BEFORE\n",
    );
    write_fixture(
        workspace.path(),
        ".opencode/skills/conformance/SKILL.md",
        concat!(
            "---\n",
            "name: conformance\n",
            "description: Verify the native skill tool\n",
            "---\n\n",
            "OPENCODE_SKILL_OK\n"
        ),
    );
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read",
            json!({"filePath": format!("{workspace_path}/read-target.txt")}),
        ),
        call(
            "write",
            json!({
                "filePath": format!("{workspace_path}/write-output.txt"),
                "content": "OPENCODE_WRITE_OK\n"
            }),
        ),
        call(
            "edit",
            json!({
                "filePath": format!("{workspace_path}/edit-target.txt"),
                "oldString": "OPENCODE_EDIT_BEFORE",
                "newString": "OPENCODE_EDIT_AFTER"
            }),
        ),
        call(
            "bash",
            json!({
                "command": format!("printf OPENCODE_BASH_OK > '{workspace_path}/bash-output.txt'")
            }),
        ),
        call("glob", json!({"pattern": "*.txt", "path": workspace_path})),
        call(
            "grep",
            json!({"pattern": "OPENCODE_READ_OK", "path": workspace_path}),
        ),
        call(
            "todowrite",
            json!({
                "todos": [{
                    "content": "Verify OpenCode conformance",
                    "status": "completed",
                    "priority": "high"
                }]
            }),
        ),
        call(
            "webfetch",
            json!({"url": "{{fixture_url}}", "format": "text"}),
        ),
        call("skill", json!({"name": "conformance"})),
        call(
            "task",
            json!({
                "description": "Verify child agent",
                "prompt": "Reply exactly OPENCODE_SUBAGENT_OK without using tools.",
                "subagent_type": "general"
            }),
        ),
    ];
    run_round_trip(
        "opencode",
        [
            "run",
            "--pure",
            "--format",
            "json",
            "--auto",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_OPENCODE_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "OPENCODE_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "OPENCODE_EDIT_AFTER");
    assert_file(workspace.path(), "bash-output.txt", "OPENCODE_BASH_OK");
}
