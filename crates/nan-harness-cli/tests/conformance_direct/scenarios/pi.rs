use super::support::*;

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
