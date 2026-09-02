use super::assertions::assert_hermes_inventory;
use super::execution::inventory;
use super::fixtures::{HERMES_OPTIONAL_CREDENTIALS_CLEARED, OPENCLAW_MEDIA_CREDENTIALS_CLEARED};
use nan_harness_test_support::conformance::assert_inventory;

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
            "--source",
            "tool",
            "--max-turns",
            "2",
        ],
        HERMES_OPTIONAL_CREDENTIALS_CLEARED,
    )
    .await;
    assert_hermes_inventory(&inventory);
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
#[ignore = "requires the pinned OMP executable"]
async fn omp_native_inventory_reaches_nan() {
    let inventory = inventory(
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
            "read,bash,edit,write,grep,glob",
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
            "web_search",
            "write",
        ],
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
            "web_search",
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
        OPENCLAW_MEDIA_CREDENTIALS_CLEARED,
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "agents_list",
            "apply_patch",
            "ask_user",
            "automations",
            "browser",
            "canvas",
            "computer",
            "conversations_list",
            "conversations_send",
            "conversations_turn",
            "create_goal",
            "dashboard",
            "dir_fetch",
            "dir_list",
            "edit",
            "exec",
            "file_fetch",
            "file_write",
            "gateway",
            "get_goal",
            "intent",
            "memory_get",
            "memory_search",
            "message",
            "mobile_ui",
            "node_inference",
            "nodes",
            "openclaw",
            "portal",
            "process",
            "progress_card",
            "read",
            "secrets",
            "session_status",
            "sessions",
            "sessions_history",
            "sessions_list",
            "sessions_search",
            "sessions_send",
            "sessions_spawn",
            "sessions_yield",
            "skill_workshop",
            "subagents",
            "terminal",
            "transcripts",
            "tts",
            "update_goal",
            "view_image",
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
#[ignore = "requires the pinned Kimi Code executable"]
async fn kimi_code_native_inventory_reaches_nan() {
    let inventory = inventory(
        "kimi-code",
        [
            "--prompt",
            "Reply exactly NAN_HARNESS_DIRECT_INVENTORY_OK without using tools.",
            "--output-format",
            "stream-json",
        ],
        &[],
    )
    .await;
    assert_inventory(
        &inventory,
        &[
            "Agent",
            "AgentSwarm",
            "AskUserQuestion",
            "Bash",
            "CreateGoal",
            "CronCreate",
            "CronDelete",
            "CronList",
            "Edit",
            "EnterPlanMode",
            "ExitPlanMode",
            "FetchURL",
            "GetGoal",
            "Glob",
            "Grep",
            "Read",
            "ReadMediaFile",
            "SetGoalBudget",
            "Skill",
            "TaskList",
            "TaskOutput",
            "TaskStop",
            "TodoList",
            "UpdateGoal",
            "WaitFor",
            "Write",
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
