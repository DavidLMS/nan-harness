use super::support::{Fixture, MARKER, executable};
use nan_harness_test_support::conformance::assert_success;
use serde_json::json;

#[tokio::test]
#[ignore = "requires Qwen Code >=0.25; NAN_REASONING_QWEN_EXECUTABLE selects the binary"]
async fn qwen_persistent_reasoning_reaches_the_native_wire() {
    for (model, selection, expected) in [
        ("qwen3.6", None, "high"),
        ("qwen3.6", Some("off"), "none"),
        ("qwen3.6", Some("high"), "high"),
        ("qwen3.6", Some("max"), "max"),
        ("gemma4", Some("off"), "none"),
        ("glm5.3-flash", None, "medium"),
        ("glm5.3-flash", Some("off"), "medium"),
        ("glm5.3-flash", Some("max"), "max"),
    ] {
        let fixture = Fixture::new().await;
        fixture.configure("qwen").await;
        let mut settings = fixture.read_json(".qwen/settings.json");
        settings["model"] = json!({"name": model});
        if selection == Some("off") {
            for entry in settings["modelProviders"]["openai"].as_array_mut().unwrap() {
                if entry["id"] == model {
                    entry["generationConfig"]["reasoning"] = json!(false);
                }
            }
        } else if let Some(effort) = selection {
            settings["model"]["reasoningEffort"] = json!(effort);
        }
        fixture.write_json(".qwen/settings.json", &settings);
        let output = fixture
            .command(executable("qwen"))
            .args([
                "--safe-mode",
                "--auth-type",
                "openai",
                "--model",
                model,
                "--max-session-turns",
                "1",
                "--max-wall-time",
                "30s",
                "--output-format",
                "json",
                "--system-prompt",
                "Reply without tools.",
                "-p",
                "Reply without tools.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
        fixture.assert_effort(model, Some(expected));
    }
}

#[tokio::test]
#[ignore = "requires Qwen Code >=0.25; verifies MiMo's distinct thinking toggle"]
async fn qwen_preserves_mimo_toggle_wire_control() {
    for enabled in [None, Some(false), Some(true)] {
        let fixture = Fixture::new().await;
        fixture.configure("qwen").await;
        let mut settings = fixture.read_json(".qwen/settings.json");
        if let Some(enabled) = enabled {
            for entry in settings["modelProviders"]["openai"].as_array_mut().unwrap() {
                if entry["id"] == "mimo-v2.6-flash" {
                    entry["generationConfig"]["reasoning"] = json!(enabled);
                }
            }
        }
        fixture.write_json(".qwen/settings.json", &settings);
        let output = fixture
            .command(executable("qwen"))
            .args([
                "--safe-mode",
                "--auth-type",
                "openai",
                "--model",
                "mimo-v2.6-flash",
                "--max-session-turns",
                "1",
                "--max-wall-time",
                "30s",
                "--output-format",
                "json",
                "-p",
                "Reply without tools.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        let requests = fixture.captured();
        let body = requests
            .iter()
            .find(|r| r["model"] == "mimo-v2.6-flash")
            .expect("MiMo request");
        assert_eq!(
            body["chat_template_kwargs"]["enable_thinking"].as_bool(),
            Some(enabled.unwrap_or(true)),
            "unexpected MiMo toggle"
        );
        assert!(body.get("reasoning_effort").is_none());
    }
}

#[tokio::test]
#[ignore = "requires Qwen Code >=0.25; verifies managed tool replay with the updated capabilities"]
async fn qwen_managed_reasoning_preserves_tool_round_trip() {
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
    let fixture = Fixture::new().await;
    let target = fixture.home.join("read-target.txt");
    std::fs::write(&target, "NAN_REASONING_TOOL_CONTENT").unwrap();
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        "read_file",
        json!({"file_path":target}),
        MARKER,
    ))
    .await
    .unwrap();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_nan-harness"))
        .args(["qwen", "--model", "qwen3.6", "--executable"])
        .args([executable("qwen")])
        .args([
            "--provider-base-url",
            provider.base_url(),
            "--no-search",
            "--",
            "--safe-mode",
            "--prompt",
            "Read the fixture file using the requested tool.",
            "--output-format",
            "json",
        ])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    let requests = provider.chat_requests();
    assert!(
        provider.completed(),
        "{}\nrequests: {}",
        output.diagnostic(),
        requests.len()
    );
    assert!(
        requests.iter().any(
            |body| body["messages"]
                .as_array()
                .is_some_and(
                    |messages| messages.iter().any(|message| message["role"] == "tool"
                        && message["content"]
                            .to_string()
                            .contains("NAN_REASONING_TOOL_CONTENT"))
                )
        )
    );
    assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
    provider.shutdown().await.unwrap();
}
