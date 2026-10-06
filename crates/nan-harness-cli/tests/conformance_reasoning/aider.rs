use super::support::{Fixture, executable};
use nan_harness_test_support::conformance::assert_success;

#[tokio::test]
#[ignore = "requires Aider; NAN_REASONING_AIDER_EXECUTABLE selects the binary"]
async fn aider_managed_accepts_budgeted_model_settings() {
    for (model, effort) in [("qwen3.6", "max"), ("qwen3.6", "none"), ("gemma4", "none")] {
        let fixture = Fixture::new().await;
        let output = fixture
            .managed("aider", model)
            .args([
                "--no-chat-gateway",
                "--",
                "--reasoning-effort",
                effort,
                "--no-git",
                "--no-auto-commits",
                "--no-check-update",
                "--no-show-release-notes",
                "--no-analytics",
                "--yes-always",
                "--message",
                "Reply without editing files.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        fixture.assert_effort(model, Some(effort));
        assert!(
            !output
                .stderr
                .contains("does not support 'reasoning_effort'")
        );
    }
}

#[tokio::test]
#[ignore = "requires Aider; checks the persistent settings through the real client"]
async fn aider_persistent_accepts_budgeted_model_settings() {
    let fixture = Fixture::new().await;
    fixture.configure("aider").await;
    let output = fixture
        .command(executable("aider"))
        .args([
            "--model",
            "nan/gemma4",
            "--weak-model",
            "nan/gemma4",
            "--editor-model",
            "nan/gemma4",
            "--reasoning-effort",
            "none",
            "--no-git",
            "--no-auto-commits",
            "--no-check-update",
            "--no-show-release-notes",
            "--no-analytics",
            "--yes-always",
            "--message",
            "Reply without editing files.",
        ])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    fixture.assert_effort("gemma4", Some("none"));
}
