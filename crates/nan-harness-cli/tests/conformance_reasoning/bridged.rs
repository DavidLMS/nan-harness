use super::support::{Fixture, MARKER};
use nan_harness_test_support::conformance::assert_success;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires Codex; NAN_REASONING_CODEX_EXECUTABLE selects the binary"]
async fn codex_accepts_adaptive_catalog_and_maximum_effort() {
    for (model, effort, expected) in [
        ("deepseek-v4-flash", None, None),
        ("qwen3.6", Some("none"), Some("none")),
        ("glm5.3-flash", Some("xhigh"), Some("max")),
    ] {
        let fixture = Fixture::new().await;
        let command = if let Some(effort) = effort {
            let selection = if effort == "none" {
                json!({"kind":"toggle","value":false})
            } else {
                json!({"kind":"effort","value":"max"})
            };
            fixture.remembered("codex", model, &selection)
        } else {
            fixture.managed("codex", model)
        }
        .args([
            "--",
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--json",
        ]);
        let output = command.args(["Reply without tools."]).run().await.unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
        fixture.assert_effort(model, expected);
    }
}

#[tokio::test]
#[ignore = "requires ZCode; NAN_REASONING_ZCODE_EXECUTABLE selects the binary"]
async fn zcode_evaluates_generated_reasoning_option_maps() {
    for (model, selection, expected) in [
        ("qwen3.6", Value::Null, None),
        (
            "gemma4",
            json!({"kind":"toggle", "value":false}),
            Some("none"),
        ),
        (
            "qwen3.6",
            json!({"kind":"effort", "value":"max"}),
            Some("max"),
        ),
        (
            "glm5.3-flash",
            json!({"kind":"effort", "value":"low"}),
            Some("low"),
        ),
    ] {
        let fixture = Fixture::new().await;
        let output = fixture
            .remembered("zcode", model, &selection)
            .args([
                "--",
                "--locale",
                "en-US",
                "--no-color",
                "--prompt",
                "Reply without tools.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
        fixture.assert_effort(model, expected);
    }
}
