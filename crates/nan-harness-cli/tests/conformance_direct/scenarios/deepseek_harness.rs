use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned DeepSeek Harness executable"]
#[allow(clippy::too_many_lines)]
async fn deepseek_harness_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "DSH_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "DSH_EDIT_BEFORE\n");
    write_fixture(
        workspace.path(),
        ".agents/skills/conformance/SKILL.md",
        concat!(
            "---\n",
            "name: conformance\n",
            "description: Verify DeepSeek Harness skill loading\n",
            "---\n\n",
            "DSH_SKILL_OK\n"
        ),
    );
    write_png(workspace.path(), "image.png");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read",
            json!({"file_path": format!("{workspace_path}/edit-target.txt")}),
        ),
        call(
            "write",
            json!({
                "file_path": format!("{workspace_path}/write-output.txt"),
                "content": "DSH_WRITE_OK\n"
            }),
        ),
        call(
            "edit",
            json!({
                "file_path": format!("{workspace_path}/edit-target.txt"),
                "old_string": "DSH_EDIT_BEFORE",
                "new_string": "DSH_EDIT_AFTER"
            }),
        ),
        call(
            "str_replace_editor",
            json!({
                "command": "create",
                "path": format!("{workspace_path}/editor-output.txt"),
                "file_text": "DSH_EDITOR_OK\n"
            }),
        ),
        call(
            "bash",
            json!({
                "command": "printf DSH_BASH_OK > bash-output.txt",
                "description": "Write deterministic bash fixture",
                "workdir": workspace_path
            }),
        ),
        call("glob", json!({"pattern": "*.txt", "path": workspace_path})),
        call(
            "grep",
            json!({"pattern": "DSH_READ_OK", "path": workspace_path}),
        ),
        call(
            "read_image",
            json!({"file_path": format!("{workspace_path}/image.png")}),
        ),
        call(
            "todo_write",
            json!({
                "todos": [{
                    "content": "Verify DeepSeek Harness tools",
                    "status": "completed"
                }]
            }),
        ),
        call("skill", json!({"name": "conformance"})),
        call(
            "workflow",
            json!({
                "script": "return { ok: true, marker: 'DSH_WORKFLOW_OK' };",
                "meta": {
                    "name": "conformance-workflow",
                    "description": "Verify the workflow runtime"
                }
            }),
        ),
        call(
            "subagent",
            json!({
                "description": "Verify foreground child",
                "prompt": "Reply exactly DSH_SUBAGENT_OK without using tools.",
                "run_in_background": false
            }),
        ),
        call(
            "subagent_fork",
            json!({
                "description": "Verify forked child",
                "prompt": "Reply exactly DSH_FORK_OK without using tools.",
                "run_in_background": false
            }),
        ),
        call(
            "subagent",
            json!({
                "description": "Verify continuable child",
                "prompt": "Reply exactly DSH_BACKGROUND_OK without using tools.",
                "run_in_background": true
            }),
        ),
        call(
            "send_message",
            json!({
                "subagent_id": "{{result_id:13}}",
                "message": "Reply exactly DSH_FOLLOWUP_OK without using tools."
            }),
        ),
        call("interrupt_agent", json!({"agent_id": "{{result_id:13}}"})),
        call("list_agents", json!({"scope": "children"})),
        call(
            "bash",
            json!({
                "command": "sleep 30",
                "description": "Start cancellable background fixture",
                "run_in_background": true,
                "workdir": workspace_path
            }),
        ),
        call(
            "job_output",
            json!({"job_id": "{{result_id:17}}", "wait": false}),
        ),
        call("job_list", json!({})),
        call(
            "job_kill",
            json!({"job_id": "{{result_id:17}}", "reason": "Conformance complete"}),
        ),
        call(
            "create_goal",
            json!({
                "objective": "Verify the goal lifecycle and then pause it",
                "max_goal_rounds": 3
            }),
        ),
        call("get_goal", json!({})),
        call(
            "update_goal",
            json!({
                "goal_id": "{{result_id:22}}",
                "revision": 1,
                "action": "pause"
            }),
        ),
        call(
            "ralph",
            json!({
                "objective": "Return a deterministic conformance result",
                "maxRounds": 1
            }),
        ),
    ];
    run_round_trip(
        "deepseek-harness",
        [
            "--profile",
            "headless",
            "Complete this long deterministic native tool conformance objective.",
        ],
        &[("DSH_PERMISSION_MODE", "danger-full-access")],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_DSH_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "DSH_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "DSH_EDIT_AFTER");
    assert_file(workspace.path(), "editor-output.txt", "DSH_EDITOR_OK");
    assert_file(workspace.path(), "bash-output.txt", "DSH_BASH_OK");
}
