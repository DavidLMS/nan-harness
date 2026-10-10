use super::support::{Fixture, MARKER, executable};
use nan_harness_test_support::conformance::assert_success;
use serde_json::json;

#[tokio::test]
#[ignore = "requires Prime Agent >=0.10; NAN_REASONING_PRIME_AGENT_EXECUTABLE selects the binary"]
async fn prime_native_and_managed_reasoning_reach_the_expected_wire() {
    for persistent in [false, true] {
        for (model, provider, thinking, effort, toggle) in [
            ("qwen3.6", "nan", "off", Some("none"), None),
            ("qwen3.6", "nan", "max", Some("max"), None),
            ("glm5.3-flash", "nan", "high", Some("high"), None),
            ("mimo-v2.6-flash", "nan-thinking", "off", None, Some(false)),
            ("mimo-v2.6-flash", "nan-thinking", "high", None, Some(true)),
            ("deepseek-v4-flash", "nan", "off", None, None),
        ] {
            let fixture = Fixture::new().await;
            let command = if persistent {
                fixture.configure("prime-agent").await;
                fixture
                    .command(executable("prime-agent"))
                    .env("NAN_API_KEY", "")
                    .args(["--provider", provider, "--model", model])
            } else {
                fixture.write_json(
                    ".prime/agent/auth.json",
                    &json!({
                        "nan": {"type": "api_key", "key": "stale-synthetic-key"}
                    }),
                );
                fixture.managed("prime-agent", model).args(["--"])
            };
            let output = command
                .env("PI_OFFLINE", "1")
                .args([
                    "--mode",
                    "json",
                    "--print",
                    "--no-session",
                    "--no-skills",
                    "--no-context-files",
                    "--thinking",
                    thinking,
                    "--daemon-socket",
                ])
                .args([fixture.home.join("prime-reasoning.sock").into_os_string()])
                .args(["Reply without tools."])
                .run()
                .await
                .unwrap();
            assert_success(&output);
            assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
            fixture.assert_effort(model, effort);
            for request in fixture.captured() {
                assert_eq!(
                    request
                        .pointer("/chat_template_kwargs/enable_thinking")
                        .and_then(serde_json::Value::as_bool),
                    toggle
                );
            }
            if !persistent {
                assert_eq!(
                    fixture.read_json(".prime/agent/auth.json")["nan"]["key"],
                    "stale-synthetic-key"
                );
            }
        }
    }
}
