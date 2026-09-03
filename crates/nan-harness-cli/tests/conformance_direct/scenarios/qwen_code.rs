use super::support::*;

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
