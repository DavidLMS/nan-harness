use super::support::*;

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
