use super::assertions::assert_hermes_inventory;
use super::execution::{
    harness_command, inventory, run_controlled_tool, run_openclaw_yield_tool, run_round_trip,
};
use super::fixtures::{
    HERMES_OPTIONAL_CREDENTIALS_CLEARED, OPENCLAW_MEDIA_CREDENTIALS_CLEARED, write_png,
};
use nan_harness_test_support::assertions::assert_aider_edit_protocol;
use nan_harness_test_support::conformance::{assert_file, assert_success, call, write_fixture};
use nan_harness_test_support::scripted_provider::{
    ProviderScenario, ScriptedProvider, ScriptedToolCall,
};
use serde_json::json;
use std::ffi::OsString;
use std::path::Path;

#[tokio::test]
#[ignore = "requires the pinned Aider executable"]
async fn aider_native_edit_protocol_reaches_nan() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "edit-target.txt", "AIDER_EDIT_BEFORE\n");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(concat!(
        "edit-target.txt\n",
        "```text\n",
        "AIDER_EDIT_AFTER\n",
        "```\n"
    )))
    .await
    .expect("scripted provider should start");
    let arguments = vec![
        OsString::from("aider"),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
        OsString::from("--message"),
        OsString::from("Replace the entire file with AIDER_EDIT_AFTER."),
        OsString::from("--yes-always"),
        OsString::from("--no-auto-commits"),
        OsString::from("--no-git"),
        OsString::from("--edit-format"),
        OsString::from("whole"),
        OsString::from("--no-show-model-warnings"),
        OsString::from("--no-check-update"),
        OsString::from("--map-tokens"),
        OsString::from("0"),
        OsString::from("edit-target.txt"),
    ];
    let output = harness_command("aider", workspace.path(), arguments, &[])
        .run()
        .await
        .expect("nan-harness should complete before the timeout");
    assert_success(&output);
    let requests = provider.chat_requests();
    assert_aider_edit_protocol(
        &output,
        &requests,
        &workspace.path().join("edit-target.txt"),
        "AIDER_EDIT_BEFORE\n",
        "AIDER_EDIT_AFTER",
    )
    .unwrap_or_else(|error| panic!("Aider edit protocol failed: {error}"));
    assert!(
        provider.completed(),
        "Aider should receive the final response"
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned Goose executable"]
async fn goose_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "edit-target.txt", "GOOSE_EDIT_BEFORE\n");
    write_png(workspace.path(), "image.png");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "write",
            json!({
                "path": format!("{workspace_path}/write-output.txt"),
                "content": "GOOSE_WRITE_OK\n"
            }),
        ),
        call(
            "edit",
            json!({
                "path": format!("{workspace_path}/edit-target.txt"),
                "before": "GOOSE_EDIT_BEFORE",
                "after": "GOOSE_EDIT_AFTER"
            }),
        ),
        call(
            "shell",
            json!({
                "command": "printf GOOSE_SHELL_OK > shell-output.txt",
                "timeout_secs": 5
            }),
        ),
        call("tree", json!({"path": workspace_path, "depth": 2})),
        call(
            "read_image",
            json!({"source": format!("{workspace_path}/image.png"), "crop": null}),
        ),
    ];
    run_round_trip(
        "goose",
        [
            "run",
            "--no-profile",
            "--no-session",
            "--with-builtin",
            "developer",
            "--output-format",
            "json",
            "--text",
            "Complete this deterministic native tool conformance objective.",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_GOOSE_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "GOOSE_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "GOOSE_EDIT_AFTER");
    assert_file(workspace.path(), "shell-output.txt", "GOOSE_SHELL_OK");
}

#[tokio::test]
#[ignore = "requires the pinned Qwen Code executable"]
async fn qwen_code_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "QWEN_READ_OK\n");
    write_fixture(
        workspace.path(),
        ".qwen/skills/conformance/SKILL.md",
        concat!(
            "---\n",
            "name: conformance\n",
            "description: Verify the native Qwen Code skill tool\n",
            "---\n\n",
            "QWEN_SKILL_OK\n"
        ),
    );
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read_file",
            json!({"file_path": format!("{workspace_path}/read-target.txt")}),
        ),
        call("list_directory", json!({"path": workspace_path})),
        call("glob", json!({"pattern": "*.txt", "path": workspace_path})),
        call(
            "grep_search",
            json!({"pattern": "QWEN_READ_OK", "path": workspace_path}),
        ),
        call(
            "todo_write",
            json!({
                "todos": [{
                    "id": "qwen-conformance",
                    "content": "Verify Qwen Code tools",
                    "status": "completed"
                }]
            }),
        ),
        call(
            "tool_search",
            json!({"query": "select:read_file", "max_results": 3}),
        ),
        call("skill", json!({"skill": "conformance"})),
        call(
            "agent",
            json!({
                "description": "Verify child agent",
                "prompt": "Reply exactly QWEN_SUBAGENT_OK without using tools.",
                "subagent_type": "general-purpose",
                "run_in_background": false
            }),
        ),
        call("list_agents", json!({})),
        call("get_goal", json!({})),
        call(
            "update_goal",
            json!({
                "status": "complete",
                "reason": "The deterministic conformance sequence is complete.",
                "evidenceRefs": ["missing-without-an-active-goal"]
            }),
        ),
    ];
    run_round_trip(
        "qwen-code",
        [
            "--safe-mode",
            "--prompt",
            "Complete the deterministic native tool conformance sequence.",
            "--output-format",
            "json",
        ],
        &[],
        &workspace,
        calls,
        &["update_goal"],
        "NAN_HARNESS_QWEN_TOOLS_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable and network access"]
async fn kimi_code_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "KIMI_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "KIMI_EDIT_BEFORE\n");
    write_fixture(
        workspace.path(),
        ".conformance-skills/conformance/SKILL.md",
        concat!(
            "---\n",
            "name: conformance\n",
            "description: Verify the native Kimi Code skill tool\n",
            "---\n\n",
            "KIMI_SKILL_OK\n"
        ),
    );
    write_png(workspace.path(), "image.png");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call("Read", json!({"path": "read-target.txt"})),
        call(
            "Write",
            json!({"path": "write-output.txt", "content": "KIMI_WRITE_OK\n"}),
        ),
        call("Read", json!({"path": "edit-target.txt"})),
        call(
            "Edit",
            json!({
                "path": "edit-target.txt",
                "old_string": "KIMI_EDIT_BEFORE",
                "new_string": "KIMI_EDIT_AFTER"
            }),
        ),
        call("Glob", json!({"pattern": "*.txt", "path": workspace_path})),
        call(
            "Grep",
            json!({"pattern": "KIMI_READ_OK", "path": "read-target.txt"}),
        ),
        call(
            "Bash",
            json!({
                "command": "printf KIMI_BASH_OK > bash-output.txt"
            }),
        ),
        call("ReadMediaFile", json!({"path": "image.png"})),
        call("FetchURL", json!({"url": "https://example.com/"})),
        call("Skill", json!({"skill": "conformance"})),
        call(
            "TodoList",
            json!({"todos": [{"title": "Verify Kimi tools", "status": "done"}]}),
        ),
        call(
            "CronCreate",
            json!({
                "cron": "0 0 1 1 *",
                "prompt": "Kimi conformance reminder",
                "recurring": false
            }),
        ),
        call("CronList", json!({})),
        call("CronDelete", json!({"id": "{{result_id:11}}"})),
    ];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Complete the deterministic native tool conformance sequence.",
            "--output-format",
            "stream-json",
            "--skills-dir",
            ".conformance-skills",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_KIMI_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "KIMI_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "KIMI_EDIT_AFTER");
    assert_file(workspace.path(), "bash-output.txt", "KIMI_BASH_OK");
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_native_agents_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call(
            "Agent",
            json!({
                "prompt": "Reply exactly KIMI_AGENT_OK without using tools.",
                "description": "Verify child agent",
                "run_in_background": false
            }),
        ),
        call(
            "AgentSwarm",
            json!({
                "description": "Verify agent swarm",
                "prompt_template": "Reply exactly KIMI_SWARM_OK for {{item}} without using tools.",
                "items": ["first", "second"]
            }),
        ),
    ];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Complete the deterministic native agent conformance sequence.",
            "--output-format",
            "stream-json",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_KIMI_AGENTS_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_native_goal_lifecycle_completes_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call(
            "CreateGoal",
            json!({
                "objective": "Complete the deterministic Kimi goal conformance sequence",
                "completionCriterion": "Every scripted goal tool returns successfully"
            }),
        ),
        call("GetGoal", json!({})),
        call("SetGoalBudget", json!({"value": 10, "unit": "turns"})),
        call("UpdateGoal", json!({"status": "complete"})),
    ];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Create and complete the explicitly requested deterministic goal.",
            "--output-format",
            "stream-json",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_KIMI_GOAL_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_native_plan_lifecycle_completes_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call("EnterPlanMode", json!({})),
        call(
            "Write",
            json!({
                "path": "{{result_id:0}}",
                "content": "# Conformance plan\n\n1. Exit plan mode successfully.\n"
            }),
        ),
        call("ExitPlanMode", json!({})),
    ];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Create and approve the deterministic native plan.",
            "--output-format",
            "stream-json",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_KIMI_PLAN_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_native_background_tasks_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![
        call(
            "Bash",
            json!({
                "command": "sleep 30",
                "description": "Kimi task conformance",
                "run_in_background": true,
                "disable_timeout": true
            }),
        ),
        call(
            "WaitFor",
            json!({"timeout": 1, "task_id": "{{result_id:0}}"}),
        ),
        call("TaskList", json!({})),
        call("TaskOutput", json!({"task_id": "{{result_id:0}}"})),
        call(
            "TaskStop",
            json!({
                "task_id": "{{result_id:0}}",
                "reason": "Conformance sequence completed"
            }),
        ),
    ];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Complete the deterministic native background task sequence.",
            "--output-format",
            "stream-json",
        ],
        &[],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_KIMI_TASKS_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_prompt_mode_handles_question_rejection() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let calls = vec![call(
        "AskUserQuestion",
        json!({
            "questions": [{
                "question": "Which conformance option should be selected?",
                "header": "Test",
                "options": [
                    {"label": "First (Recommended)", "description": "Use the first deterministic option."},
                    {"label": "Second", "description": "Use the second deterministic option."}
                ],
                "multi_select": false
            }],
            "background": true
        }),
    )];
    run_round_trip(
        "kimi-code",
        [
            "--prompt",
            "Attempt the explicitly requested question in prompt mode, then continue cleanly if it is rejected.",
            "--output-format",
            "stream-json",
        ],
        &[],
        &workspace,
        calls,
        &["AskUserQuestion"],
        "NAN_HARNESS_KIMI_QUESTION_OK",
    )
    .await;
}

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

#[tokio::test]
#[ignore = "requires the pinned Pi executable"]
async fn pi_native_tools_complete_round_trips() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_fixture(workspace.path(), "read-target.txt", "PI_READ_OK\n");
    write_fixture(workspace.path(), "edit-target.txt", "PI_EDIT_BEFORE\n");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call(
            "read",
            json!({"path": format!("{workspace_path}/read-target.txt")}),
        ),
        call(
            "write",
            json!({
                "path": format!("{workspace_path}/write-output.txt"),
                "content": "PI_WRITE_OK\n"
            }),
        ),
        call(
            "edit",
            json!({
                "path": format!("{workspace_path}/edit-target.txt"),
                "edits": [{"oldText": "PI_EDIT_BEFORE", "newText": "PI_EDIT_AFTER"}]
            }),
        ),
        call(
            "bash",
            json!({
                "command": format!("printf PI_BASH_OK > '{workspace_path}/bash-output.txt'")
            }),
        ),
        call(
            "grep",
            json!({"pattern": "PI_READ_OK", "path": workspace_path}),
        ),
        call("find", json!({"pattern": "*.txt", "path": workspace_path})),
        call("ls", json!({"path": workspace_path})),
    ];
    run_round_trip(
        "pi",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--tools",
            "read,bash,edit,write,grep,find,ls",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[("PI_OFFLINE", "1")],
        &workspace,
        calls,
        &[],
        "NAN_HARNESS_PI_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "write-output.txt", "PI_WRITE_OK");
    assert_file(workspace.path(), "edit-target.txt", "PI_EDIT_AFTER");
    assert_file(workspace.path(), "bash-output.txt", "PI_BASH_OK");
}

#[tokio::test]
#[ignore = "requires the pinned OMP executable"]
async fn omp_native_write_completes_a_round_trip() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let workspace_path = workspace.path().to_string_lossy();
    run_round_trip(
        "omp",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-rules",
            "--no-lsp",
            "--no-title",
            "--tools",
            "write",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[],
        &workspace,
        vec![call(
            "write",
            json!({
                "path": format!("{workspace_path}/omp-output.txt"),
                "content": "OMP_WRITE_OK\n"
            }),
        )],
        &[],
        "NAN_HARNESS_OMP_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "omp-output.txt", "OMP_WRITE_OK");
}

#[tokio::test]
#[ignore = "requires the pinned OMP executable"]
async fn omp_without_authenticated_search_falls_back_to_nan() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    run_round_trip(
        "omp",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-rules",
            "--no-lsp",
            "--no-title",
            "--tools",
            "web_search",
            "Complete the deterministic NaN search fallback check.",
        ],
        &[],
        &workspace,
        vec![call(
            "web_search",
            json!({"query": "nan-harness OMP conformance", "limit": 1}),
        )],
        &["web_search"],
        "NAN_HARNESS_OMP_SEARCH_OK",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires the pinned Prime Agent executable and IPython"]
async fn prime_agent_ipython_completes_a_round_trip() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let output_path = workspace.path().join("prime-output.txt");
    let output_path_literal = serde_json::to_string(&output_path.to_string_lossy())
        .expect("Prime output path should serialize as a JSON string literal");
    run_round_trip(
        "prime-agent",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--tools",
            "ipython",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[("PI_OFFLINE", "1")],
        &workspace,
        vec![call(
            "ipython",
            json!({
                "code": format!(
                    "from pathlib import Path; output_path = Path({output_path_literal}); output_path.write_text('PRIME_IPYTHON_OK', encoding='utf-8'); output_path.read_text(encoding='utf-8')"
                )
            }),
        )],
        &[],
        "NAN_HARNESS_PRIME_TOOLS_OK",
    )
    .await;
    assert_file(workspace.path(), "prime-output.txt", "PRIME_IPYTHON_OK");
}

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
