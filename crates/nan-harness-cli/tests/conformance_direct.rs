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
    let workspace_path = workspace.path().to_string_lossy();
    let skill = concat!(
        "---\n",
        "name: conformance\n",
        "description: Verify Hermes skill tools\n",
        "---\n\n",
        "HERMES_SKILL_OK\n"
    );
    let calls = vec![
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
    ];
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
        .expect("NaN Harness should complete before the timeout");
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
    for index in 0..calls.len() {
        let tool_call_id = format!("call_nan_harness_conformance_{index}");
        let result = tool_result(&requests, &tool_call_id).unwrap_or_else(|| {
            panic!(
                "{harness} did not return a result for {} ({tool_call_id})\n{}",
                calls[index].name,
                output.diagnostic()
            )
        });
        let failed = result.trim_start().starts_with("Error:");
        assert!(
            !failed || allowed_errors.contains(&calls[index].name.as_str()),
            "{harness} tool '{}' failed: {result}",
            calls[index].name
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
        arguments.splice(
            separator + 1..separator + 1,
            [
                OsString::from("--daemon-socket"),
                prime_daemon_socket(workspace).into_os_string(),
            ],
        );
    }
    let mut command = TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace)
        .args(arguments)
        .env("NAN_API_KEY", "nan_test_key")
        .timeout(Duration::from_secs(120));
    let isolated_home = workspace.join(".conformance-home");
    match harness {
        "opencode" => {
            command = command
                .env("XDG_CONFIG_HOME", isolated_home.join("config"))
                .env("XDG_DATA_HOME", isolated_home.join("data"))
                .env("XDG_CACHE_HOME", isolated_home.join("cache"));
        }
        "hermes" => command = command.env("HOME", &isolated_home),
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
                        && message.get("tool_call_id").and_then(Value::as_str)
                            == Some(tool_call_id);
                    matches.then(|| {
                        message
                            .get("content")
                            .and_then(Value::as_str)
                            .map_or_else(|| message.to_string(), ToOwned::to_owned)
                    })
                })
            })
    })
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
    let content = std::fs::read_to_string(workspace.join(relative_path))
        .expect("expected conformance file should exist");
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
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(
        output.stdout.contains(INVENTORY_MARKER),
        "{}",
        output.diagnostic()
    );
    assert!(!output.stdout.contains("NH-BRIDGE-"));
}
