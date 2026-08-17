#![cfg(unix)]

use nan_harness_test_support::scripted_provider::{
    ProviderScenario, ScriptedProvider, ScriptedToolCall,
};
use nan_harness_test_support::terminal::{TerminalCommand, TerminalOutput};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const INVENTORY_MARKER: &str = "NAN_HARNESS_DIRECT_INVENTORY_OK";

#[tokio::test]
#[ignore = "requires the pinned OpenCode executable"]
async fn opencode_native_inventory_reaches_nan() {
    let inventory = inventory(
        "opencode",
        [
            "run",
            "--pure",
            "--format",
            "json",
            "--auto",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "bash",
            "edit",
            "glob",
            "grep",
            "read",
            "skill",
            "task",
            "todowrite",
            "webfetch",
            "write",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Hermes executable"]
async fn hermes_native_inventory_reaches_nan() {
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
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "browser_exec",
            "clarify",
            "cronjob",
            "delegate_task",
            "execute_code",
            "image_generate",
            "memory",
            "patch",
            "process",
            "read_file",
            "search_files",
            "session_search",
            "skill_manage",
            "skill_view",
            "skills_list",
            "terminal",
            "text_to_speech",
            "todo",
            "write_file",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Pi executable"]
async fn pi_native_inventory_reaches_nan() {
    let inventory = inventory(
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
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[("PI_OFFLINE", "1")],
    )
    .await;
    assert_inventory(
        &inventory,
        &["bash", "edit", "find", "grep", "ls", "read", "write"],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Prime Agent executable"]
async fn prime_agent_native_inventory_reaches_nan() {
    let inventory = inventory(
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
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[("PI_OFFLINE", "1")],
    )
    .await;
    assert_inventory(&inventory, &["ipython"]);
}

#[tokio::test]
#[ignore = "requires the pinned DeepSeek Harness executable"]
async fn deepseek_harness_native_inventory_reaches_nan() {
    let inventory = inventory(
        "deepseek-harness",
        [
            "--profile",
            "headless",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[("DSH_PERMISSION_MODE", "danger-full-access")],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "bash",
            "create_goal",
            "edit",
            "exit_plan_mode",
            "get_goal",
            "glob",
            "grep",
            "interrupt_agent",
            "job_kill",
            "job_list",
            "job_output",
            "list_agents",
            "ralph",
            "read",
            "read_image",
            "send_message",
            "skill",
            "str_replace_editor",
            "subagent",
            "subagent_fork",
            "todo_write",
            "update_goal",
            "workflow",
            "write",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned OpenClaw executable"]
async fn openclaw_native_inventory_reaches_nan() {
    let inventory = inventory(
        "openclaw",
        [
            "agent",
            "--local",
            "--session-id",
            "nan-harness-inventory",
            "--message",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
            "--json",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "agents_list",
            "apply_patch",
            "browser",
            "canvas",
            "create_goal",
            "cron",
            "dir_fetch",
            "dir_list",
            "edit",
            "exec",
            "file_fetch",
            "file_write",
            "gateway",
            "get_goal",
            "image",
            "image_generate",
            "memory_get",
            "memory_search",
            "message",
            "music_generate",
            "node_inference",
            "nodes",
            "process",
            "read",
            "session_status",
            "sessions_history",
            "sessions_list",
            "sessions_send",
            "sessions_spawn",
            "sessions_yield",
            "skill_workshop",
            "subagents",
            "tts",
            "update_goal",
            "video_generate",
            "web_fetch",
            "web_search",
            "write",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Cline executable"]
async fn cline_native_inventory_reaches_nan() {
    let inventory = inventory(
        "cline",
        [
            "--json",
            "--timeout",
            "60",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "ask_question",
            "editor",
            "fetch_web_content",
            "read_files",
            "run_commands",
            "search_codebase",
            "spawn_agent",
            "team_attach_outcome_fragment",
            "team_await_runs",
            "team_broadcast",
            "team_cancel_run",
            "team_cleanup",
            "team_create_outcome",
            "team_finalize_outcome",
            "team_list_outcomes",
            "team_list_runs",
            "team_mission_log",
            "team_read_mailbox",
            "team_review_outcome_fragment",
            "team_run_task",
            "team_send_message",
            "team_shutdown_teammate",
            "team_spawn_teammate",
            "team_status",
            "team_task",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Qwen Code executable"]
async fn qwen_code_native_inventory_reaches_nan() {
    let inventory = inventory(
        "qwen-code",
        [
            "--safe-mode",
            "--prompt",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
            "--output-format",
            "json",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "agent",
            "get_goal",
            "glob",
            "grep_search",
            "list_agents",
            "list_directory",
            "read_file",
            "skill",
            "todo_write",
            "tool_search",
            "update_goal",
        ],
    );
}

#[tokio::test]
#[ignore = "requires the pinned Goose executable"]
async fn goose_native_inventory_reaches_nan() {
    let inventory = inventory(
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
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &["edit", "read_image", "shell", "tree", "write"],
    );
}

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
        OsString::from("run"),
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
        .expect("NaN Harness should complete before the timeout");
    assert_clean_success(&output);
    assert_file(workspace.path(), "edit-target.txt", "AIDER_EDIT_AFTER");
    let requests = provider.chat_requests();
    assert!(!requests.is_empty(), "Aider should reach the provider");
    assert!(
        requests
            .iter()
            .all(|request| request.get("tools").is_none()),
        "Aider should use its edit protocol instead of function tools"
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
        call("image_generate", json!({"action": "list"})),
        call("music_generate", json!({"action": "list"})),
        call("video_generate", json!({"action": "list"})),
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
        &[],
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
#[ignore = "requires the pinned OpenClaw executable and optional tool runtimes"]
async fn openclaw_environment_bound_tools_return_controlled_results() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    write_png(workspace.path(), "image.png");
    let workspace_path = workspace.path().to_string_lossy();
    let calls = vec![
        call("browser", json!({"action": "status"})),
        call(
            "canvas",
            json!({"action": "snapshot", "node": "missing-conformance-node"}),
        ),
        call("cron", json!({"action": "status"})),
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
            "image",
            json!({
                "image": format!("{workspace_path}/image.png"),
                "prompt": "Return a concise deterministic description."
            }),
        ),
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
            "cron",
            "dir_fetch",
            "dir_list",
            "file_fetch",
            "file_write",
            "gateway",
            "image",
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
#[ignore = "requires the pinned Prime Agent executable and IPython"]
async fn prime_agent_ipython_completes_a_round_trip() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
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
            json!({"code": "print('PRIME_IPYTHON_OK')"}),
        )],
        &[],
        "NAN_HARNESS_PRIME_TOOLS_OK",
    )
    .await;
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
                "context": "This is a deterministic NaN Harness conformance check."
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
    let environment = [
        ("BFL_API_KEY", ""),
        ("ELEVENLABS_API_KEY", ""),
        ("FAL_KEY", ""),
        ("OPENAI_API_KEY", ""),
        ("XAI_API_KEY", ""),
    ];
    let calls = [
        call(
            "browser_exec",
            json!({
                "code": "# Verify browser runtime\nprint('HERMES_BROWSER_OK')",
                "timeout_s": 5
            }),
        ),
        call(
            "text_to_speech",
            json!({"text": "Hermes conformance", "provider": "edge"}),
        ),
    ];

    for tool_call in calls {
        run_controlled_tool(
            "hermes",
            &harness_arguments,
            &environment,
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

async fn inventory<const N: usize>(
    harness: &str,
    harness_arguments: [&str; N],
    environment: &[(&str, &str)],
) -> BTreeSet<String> {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from("run"),
        OsString::from(harness),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.into_iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "NaN Harness should complete before the timeout: {error}\nprovider requests: {:#?}",
                provider.chat_requests()
            )
        });
    assert_success(&output);
    let requests = provider.chat_requests();
    let tools = requests
        .iter()
        .find_map(tool_names)
        .expect("the harness should advertise at least one tool");
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
    tools
}

async fn run_round_trip<const N: usize>(
    harness: &str,
    harness_arguments: [&str; N],
    environment: &[(&str, &str)],
    workspace: &tempfile::TempDir,
    calls: Vec<ScriptedToolCall>,
    allowed_errors: &[&str],
    final_marker: &str,
) {
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        calls.iter().cloned(),
        final_marker,
    ))
    .await
    .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from("run"),
        OsString::from(harness),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.into_iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .expect("NaN Harness should complete before the timeout");
    assert!(output.status.success(), "{}", output.diagnostic());
    let requests = provider.chat_requests();
    for (index, tool_call) in calls.iter().enumerate() {
        let tool_call_id = format!("call_nan_harness_conformance_{index}");
        let result = tool_result(&requests, &tool_call_id).unwrap_or_else(|| {
            panic!(
                "{harness} did not return a result for {} ({tool_call_id})\n{}",
                tool_call.name,
                output.diagnostic()
            )
        });
        let failed = tool_result_failed(&result);
        assert!(
            !failed || allowed_errors.contains(&tool_call.name.as_str()),
            "{harness} tool '{}' failed: {result}",
            tool_call.name
        );
    }
    assert!(
        output.stdout.contains(final_marker),
        "{}",
        output.diagnostic()
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

async fn run_controlled_tool(
    harness: &str,
    harness_arguments: &[&str],
    environment: &[(&str, &str)],
    workspace: &tempfile::TempDir,
    tool_call: ScriptedToolCall,
) {
    let _prime_daemon = PrimeDaemonGuard::for_harness(harness, workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        tool_call.name.clone(),
        tool_call.input,
        "NAN_HARNESS_CONTROLLED_TOOL_OK",
    ))
    .await
    .expect("scripted provider should start");
    let mut arguments = vec![
        OsString::from("run"),
        OsString::from(harness),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.iter().map(OsString::from));
    let output = harness_command(harness, workspace.path(), arguments, environment)
        .run()
        .await
        .expect("NaN Harness should complete before the timeout");
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(
        !output.stderr.contains("Traceback"),
        "{}",
        output.diagnostic()
    );
    let requests = provider.chat_requests();
    let result = tool_result(&requests, "call_nan_harness_conformance_0").unwrap_or_else(|| {
        panic!(
            "{harness} did not return a controlled result for '{}'\n{}",
            tool_call.name,
            output.diagnostic()
        )
    });
    assert!(!result.trim().is_empty(), "{}", output.diagnostic());
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

async fn run_openclaw_yield_tool(workspace: &tempfile::TempDir) {
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        "sessions_yield",
        json!({"message": "Wait for deterministic child completion."}),
        "NAN_HARNESS_OPENCLAW_YIELD_OK",
    ))
    .await
    .expect("scripted provider should start");
    let arguments = vec![
        OsString::from("run"),
        OsString::from("openclaw"),
        OsString::from("--provider-base-url"),
        OsString::from(provider.base_url()),
        OsString::from("--"),
        OsString::from("agent"),
        OsString::from("--local"),
        OsString::from("--session-id"),
        OsString::from("nan-harness-yield-tool"),
        OsString::from("--message"),
        OsString::from("Complete the controlled sessions_yield conformance check."),
        OsString::from("--json"),
    ];
    let output = harness_command("openclaw", workspace.path(), arguments, &[])
        .run()
        .await
        .expect("NaN Harness should complete before the timeout");
    assert!(output.status.success(), "{}", output.diagnostic());
    let report: Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("OpenClaw should return a JSON report: {error}"));
    assert_eq!(report.pointer("/meta/yielded"), Some(&Value::Bool(true)));
    assert_eq!(
        report.pointer("/meta/toolSummary/failures"),
        Some(&Value::from(0))
    );
    assert!(
        report
            .pointer("/meta/toolSummary/tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|tool| tool == "sessions_yield")),
        "{}",
        output.diagnostic()
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

fn harness_command(
    harness: &str,
    workspace: &Path,
    mut arguments: Vec<OsString>,
    environment: &[(&str, &str)],
) -> TerminalCommand {
    if harness == "prime-agent" {
        std::fs::create_dir_all(workspace.join(".conformance-home"))
            .expect("Prime Agent conformance home should exist");
        let separator = arguments
            .iter()
            .position(|argument| argument == "--")
            .expect("NaN Harness arguments should include a separator");
        arguments.insert(
            separator + 1,
            prime_daemon_socket(workspace).into_os_string(),
        );
        arguments.insert(separator + 1, OsString::from("--daemon-socket"));
    }
    let mut command = TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace)
        .args(arguments)
        .env("NAN_API_KEY", "nan_test_key")
        .timeout(Duration::from_mins(2));
    let isolated_home = workspace.join(".conformance-home");
    match harness {
        "opencode" => {
            command = command
                .env("XDG_CONFIG_HOME", isolated_home.join("config"))
                .env("XDG_DATA_HOME", isolated_home.join("data"))
                .env("XDG_CACHE_HOME", isolated_home.join("cache"));
        }
        "hermes" | "openclaw" | "cline" | "qwen-code" | "aider" | "goose" => {
            std::fs::create_dir_all(&isolated_home).expect("conformance home should exist");
            command = command.env("HOME", &isolated_home);
        }
        "pi" | "prime-agent" => {
            command = command.env("PI_CODING_AGENT_DIR", isolated_home.join("pi-agent"));
        }
        "deepseek-harness" => command = command.env("DSH_HOME", isolated_home.join("dsh")),
        _ => {}
    }
    for (name, value) in environment {
        command = command.env(name, value);
    }
    command
}

struct PrimeDaemonGuard {
    socket: std::path::PathBuf,
}

impl PrimeDaemonGuard {
    fn for_harness(harness: &str, workspace: &Path) -> Option<Self> {
        (harness == "prime-agent").then(|| Self {
            socket: prime_daemon_socket(workspace),
        })
    }
}

impl Drop for PrimeDaemonGuard {
    fn drop(&mut self) {
        let Ok(output) = Command::new("prime-agent")
            .args(["status", "--json"])
            .output()
        else {
            return;
        };
        let Ok(daemons) = serde_json::from_slice::<Value>(&output.stdout) else {
            return;
        };
        let Some(pid) = daemons.as_array().and_then(|entries| {
            entries.iter().find_map(|entry| {
                (entry.get("socketPath").and_then(Value::as_str)
                    == Some(self.socket.to_string_lossy().as_ref()))
                .then(|| entry.get("pid").and_then(Value::as_u64))
                .flatten()
            })
        }) else {
            return;
        };
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn prime_daemon_socket(workspace: &Path) -> std::path::PathBuf {
    workspace.join(".conformance-home/prime-agent.sock")
}

fn call(name: &str, input: Value) -> ScriptedToolCall {
    ScriptedToolCall {
        name: name.to_owned(),
        input,
        result_expected: true,
    }
}

fn tool_result(requests: &[Value], tool_call_id: &str) -> Option<String> {
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
                            .is_some_and(|actual| tool_call_ids_match(actual, tool_call_id));
                    matches.then(|| {
                        message.get("content").map_or_else(
                            || message.to_string(),
                            |content| {
                                content.as_str().map_or_else(
                                    || {
                                        content.as_array().map_or_else(
                                            || content.to_string(),
                                            |blocks| {
                                                blocks
                                                    .iter()
                                                    .map(|block| {
                                                        block
                                                            .get("text")
                                                            .and_then(Value::as_str)
                                                            .map_or_else(
                                                                || block.to_string(),
                                                                ToOwned::to_owned,
                                                            )
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            },
                                        )
                                    },
                                    ToOwned::to_owned,
                                )
                            },
                        )
                    })
                })
            })
    })
}

fn tool_call_ids_match(left: &str, right: &str) -> bool {
    left.chars()
        .filter(char::is_ascii_alphanumeric)
        .eq(right.chars().filter(char::is_ascii_alphanumeric))
}

fn tool_result_failed(result: &str) -> bool {
    if result
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

fn assert_inventory(actual: &BTreeSet<String>, expected: &[&str]) {
    let expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, &expected);
}

fn write_fixture(workspace: &Path, relative_path: &str, content: &str) {
    let path = workspace.join(relative_path);
    std::fs::create_dir_all(path.parent().expect("fixture should have a parent"))
        .expect("fixture directory should exist");
    std::fs::write(path, content).expect("fixture should be written");
}

fn write_png(workspace: &Path, relative_path: &str) {
    const ONE_PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let path = workspace.join(relative_path);
    std::fs::write(path, ONE_PIXEL_PNG).expect("PNG fixture should be written");
}

fn assert_file(workspace: &Path, relative_path: &str, expected: &str) {
    let path = workspace.join(relative_path);
    let content = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "expected conformance file '{}' should exist: {error}",
            path.display()
        )
    });
    assert!(content.contains(expected), "file content was {content:?}");
}

fn tool_names(request: &Value) -> Option<BTreeSet<String>> {
    request
        .get("tools")?
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.pointer("/function/name"))
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .filter(|tools: &BTreeSet<String>| !tools.is_empty())
}

fn assert_success(output: &TerminalOutput) {
    assert_clean_success(output);
    assert!(
        output.stdout.contains(INVENTORY_MARKER),
        "{}",
        output.diagnostic()
    );
}

fn assert_clean_success(output: &TerminalOutput) {
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(!output.stdout.contains("NH-BRIDGE-"));
}
