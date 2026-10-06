use super::support::{Fixture, MARKER, executable};
use nan_harness_test_support::conformance::assert_success;

#[tokio::test]
#[ignore = "requires OpenCode; NAN_REASONING_OPENCODE_EXECUTABLE selects the binary"]
async fn opencode_managed_variants_reach_the_native_wire() {
    opencode_variants("opencode").await;
}

#[tokio::test]
#[ignore = "requires MiMo Code; NAN_REASONING_MIMO_EXECUTABLE selects the binary"]
async fn mimo_managed_variants_reach_the_native_wire() {
    opencode_variants("mimo").await;
}

async fn opencode_variants(harness: &str) {
    for (model, selection, expected) in [
        ("qwen3.6", None, None),
        ("qwen3.6", Some("off"), Some("none")),
        ("qwen3.6", Some("max"), Some("max")),
        ("gemma4", Some("off"), Some("none")),
        ("glm5.3-flash", Some("max"), Some("max")),
    ] {
        let fixture = Fixture::new().await;
        let mut command = fixture.managed(harness, model).args([
            "--no-chat-gateway",
            "--",
            "run",
            "--pure",
            "--format",
            "json",
        ]);
        if let Some(variant) = selection {
            command = command.args(["--variant", variant]);
        }
        let output = command.args(["Reply without tools."]).run().await.unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
        fixture.assert_chat_effort(model, expected);
    }
}

#[tokio::test]
#[ignore = "requires OpenCode; checks the persistent renderer through the real client"]
async fn opencode_persistent_variants_reach_the_native_wire() {
    for effort in ["off", "max"] {
        let fixture = Fixture::new().await;
        fixture.configure("opencode").await;
        let output = fixture
            .command(executable("opencode"))
            .env("XDG_CONFIG_HOME", fixture.home.join(".config"))
            .args([
                "run",
                "--pure",
                "--format",
                "json",
                "--model",
                "nan/qwen3.6",
                "--variant",
                effort,
                "Reply without tools.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        fixture.assert_chat_effort(
            "qwen3.6",
            Some(if effort == "off" { "none" } else { effort }),
        );
    }
}

#[tokio::test]
#[ignore = "requires OpenCode and MiMo Code; checks their separate native toggle option"]
async fn opencode_and_mimo_preserve_nested_thinking_toggles() {
    for harness in ["opencode", "mimo"] {
        for (variant, enabled) in [("thinking", true), ("no-thinking", false)] {
            let fixture = Fixture::new().await;
            let output = fixture
                .managed(harness, "mimo-v2.6-flash")
                .args([
                    "--no-chat-gateway",
                    "--",
                    "run",
                    "--pure",
                    "--format",
                    "json",
                    "--variant",
                    variant,
                    "Reply without tools.",
                ])
                .run()
                .await
                .unwrap();
            assert_success(&output);
            let requests = fixture.primary_chat_requests("mimo-v2.6-flash");
            assert!(!requests.is_empty(), "missing primary chat task");
            assert!(
                requests
                    .iter()
                    .any(|body| body["chat_template_kwargs"]["enable_thinking"] == enabled),
                "{requests:?}"
            );
            assert!(
                requests
                    .iter()
                    .all(|body| body.get("enable_thinking").is_none()),
                "toggle must remain nested"
            );
        }
    }
}
