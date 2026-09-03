use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned Cline executable"]
async fn cline_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "CLINE_READ_OK\n");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read_files",
            json!({"files": [{"path": format!("{workspace_path}/read-target.txt")}]}),
        ),
        call("search_codebase", json!({"queries": ["CLINE_READ_OK"]})),
        call(
            "run_commands",
            json!({
                "commands": [format!(
                    "printf CLINE_COMMAND_OK > '{workspace_path}/command-output.txt'"
                )]
            }),
        ),
        call(
            "fetch_web_content",
            json!({
                "requests": [{
                    "url": "{{fixture_url}}",
                    "prompt": "Return the conformance marker"
                }]
            }),
        ),
        call(
            "editor",
            json!({
                "path": format!("{workspace_path}/editor-output.txt"),
                "new_text": "CLINE_EDITOR_OK\n"
            }),
        ),
        call(
            "ask_question",
            json!({
                "question": "Choose the deterministic conformance answer.",
                "options": ["Continue", "Stop"]
            }),
        ),
        call(
            "spawn_agent",
            json!({
                "systemPrompt": "Return only deterministic conformance results.",
                "task": "Reply exactly CLINE_SUBAGENT_OK without using tools."
            }),
        ),
    ];
    run_round_trip(
        "cline",
        [
            "--json",
            "--timeout",
            "90",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[],
        &workspace,
        calls,
        &["ask_question"],
        "NAN_HARNESS_CLINE_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "command-output.txt", "CLINE_COMMAND_OK");
    assert_file(workspace.path(), "editor-output.txt", "CLINE_EDITOR_OK");
}

#[tokio::test]
#[ignore = "requires the pinned Cline executable"]
#[allow(clippy::too_many_lines)]
async fn cline_team_tools_complete_lifecycle_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call(
            "team_spawn_teammate",
            json!({
                "agentId": "conformance-worker",
                "rolePrompt": "Return only deterministic conformance results."
            }),
        ),
        call("team_status", json!({})),
        call(
            "team_task",
            json!({
                "action": "create",
                "title": "Verify Cline team tools",
                "description": "Complete the deterministic team lifecycle.",
                "assignee": "conformance-worker"
            }),
        ),
        call(
            "team_run_task",
            json!({
                "agentId": "conformance-worker",
                "task": "Reply exactly CLINE_TEAM_RUN_OK without using tools.",
                "taskId": "{{result_id:2}}",
                "runMode": "async"
            }),
        ),
        call("team_list_runs", json!({"includeCompleted": true})),
        call("team_await_runs", json!({"runId": "{{result_id:3}}"})),
        call(
            "team_send_message",
            json!({
                "toAgentId": "conformance-worker",
                "subject": "Conformance",
                "body": "The deterministic run completed.",
                "taskId": "{{result_id:2}}"
            }),
        ),
        call(
            "team_broadcast",
            json!({
                "subject": "Conformance",
                "body": "The deterministic lifecycle is finishing.",
                "taskId": "{{result_id:2}}"
            }),
        ),
        call("team_read_mailbox", json!({"unreadOnly": false})),
        call(
            "team_mission_log",
            json!({
                "kind": "done",
                "summary": "Cline team conformance completed.",
                "taskId": "{{result_id:2}}",
                "evidence": ["Local scripted provider returned a tool result."]
            }),
        ),
        call(
            "team_task",
            json!({
                "action": "complete",
                "taskId": "{{result_id:2}}",
                "summary": "All deterministic team checks completed."
            }),
        ),
        call(
            "team_cancel_run",
            json!({
                "runId": "{{result_id:3}}",
                "reason": "Verify idempotent cancellation after completion."
            }),
        ),
        call(
            "team_shutdown_teammate",
            json!({
                "agentId": "conformance-worker",
                "reason": "Conformance complete."
            }),
        ),
        call(
            "team_create_outcome",
            json!({
                "title": "Cline team conformance",
                "requiredSections": ["summary"]
            }),
        ),
        call(
            "team_attach_outcome_fragment",
            json!({
                "outcomeId": "{{result_id:13}}",
                "section": "summary",
                "content": "CLINE_TEAM_OUTCOME_OK"
            }),
        ),
        call(
            "team_review_outcome_fragment",
            json!({
                "fragmentId": "{{result_id:14}}",
                "approved": true
            }),
        ),
        call(
            "team_finalize_outcome",
            json!({"outcomeId": "{{result_id:13}}"}),
        ),
        call("team_list_outcomes", json!({})),
        call("team_cleanup", json!({})),
    ];
    run_round_trip(
        "cline",
        [
            "--json",
            "--timeout",
            "90",
            "Complete the deterministic native team-tool conformance sequence.",
        ],
        &[],
        &workspace,
        calls,
        &["team_cancel_run"],
        "NAN_HARNESS_CLINE_TEAM_TOOLS_OK",
    )
    .await;
}
