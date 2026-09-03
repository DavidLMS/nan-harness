use super::support::*;

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
