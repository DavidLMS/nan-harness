use super::support::*;

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
