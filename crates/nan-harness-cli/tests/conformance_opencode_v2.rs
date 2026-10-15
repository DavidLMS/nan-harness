//! Native V2 qualification uses a real, explicitly selected executable and synthetic HTTP.
use nan_harness_test_support::conformance::{assert_file, assert_success, call, tool_result};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::TerminalCommand;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MARKER: &str = "NAN_OPENCODE_V2_OK";

fn executable() -> PathBuf {
    std::env::var_os("NAN_OPENCODE_V2_EXECUTABLE")
        .map_or_else(|| PathBuf::from("opencode2"), PathBuf::from)
}

fn isolated(program: impl Into<PathBuf>, workspace: &Path) -> TerminalCommand {
    let home = workspace.join("home");
    std::fs::create_dir_all(home.join("tmp")).unwrap();
    let mut command = TerminalCommand::new(program, workspace)
        .clear_environment()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("TEMP", home.join("tmp"))
        .env("TMP", home.join("tmp"))
        .env("OPENCODE_TEST_HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("NAN_HARNESS_CONFIG_DIR", home.join(".nan-harness"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_API_KEY", "synthetic-opencode-v2-key")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .timeout(Duration::from_mins(1));
    // Windows networking and subprocess loading require these OS locations.
    for name in ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(name) {
            command = command.env(name, value);
        }
    }
    command
}

#[tokio::test]
#[ignore = "requires OpenCode V2; NAN_OPENCODE_V2_EXECUTABLE selects the binary"]
async fn opencode_v2_alias_completes_native_write_and_read() {
    let workspace = tempfile::tempdir().unwrap();
    let target = workspace.path().join("written.txt");
    let provider = ScriptedProvider::start(
        ProviderScenario::sequence(
            [
                call("write", json!({"path":target,"content":"V2_WRITE_OK\n"})),
                call("read", json!({"path":target})),
            ],
            MARKER,
        )
        .with_api_key("synthetic-opencode-v2-key"),
    )
    .await
    .unwrap();
    let output = isolated(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args(["opencode2", "--model", "qwen3.6", "--executable"])
        .args([executable()])
        .args([
            "--provider-base-url",
            provider.base_url(),
            "--no-search",
            "--",
        ])
        .args([
            "run",
            "--format",
            "json",
            "--auto",
            "Complete the fixture tool calls.",
        ])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    assert!(provider.completed(), "{}", output.diagnostic());
    assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
    assert_file(workspace.path(), "written.txt", "V2_WRITE_OK");
    let requests = provider.chat_requests();
    let result = tool_result(&requests, "call_nan_harness_conformance_1").unwrap();
    assert!(result.contains("V2_WRITE_OK"), "{result}");
    provider.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode V2; NAN_OPENCODE_V2_EXECUTABLE selects the binary"]
async fn opencode_v2_managed_launch_preserves_model_and_authentication() {
    let workspace = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::start(
        ProviderScenario::inventory(MARKER).with_api_key("synthetic-opencode-v2-key"),
    )
    .await
    .unwrap();
    let output = isolated(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args([
            "opencode",
            "--major",
            "2",
            "--model",
            "qwen3.6",
            "--executable",
        ])
        .args([executable()])
        .args(["--provider-base-url", provider.base_url(), "--no-search"])
        .args([
            "--no-chat-gateway",
            "--",
            "run",
            "--format",
            "json",
            "Reply OK.",
        ])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
    let requests = provider.chat_requests();
    assert!(requests.iter().any(|body| {
        body["model"] == "qwen3.6"
            && body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
    }));
    provider.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode V2; NAN_OPENCODE_V2_EXECUTABLE selects the binary"]
async fn opencode_v2_persistent_key_rotation_and_reasoning_reach_the_wire() {
    let workspace = tempfile::tempdir().unwrap();
    for (refresh, key) in [
        (false, "synthetic-opencode-v2-key"),
        (true, "synthetic-rotated-key"),
    ] {
        let state = workspace.path().join("home/.nan-harness");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("nan-api-key"), key).unwrap();
        std::fs::write(
            state.join("credential.json"),
            r#"{"schemaVersion":1,"backend":"private-file"}"#,
        )
        .unwrap();
        let provider =
            ScriptedProvider::start(ProviderScenario::inventory(MARKER).with_api_key(key))
                .await
                .unwrap();
        let mut configure = isolated(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
            .env("NAN_BASE_URL", provider.base_url())
            .env("NAN_API_KEY", key)
            .args(["config", "opencode2", "--no-search"]);
        if refresh {
            configure = configure.args(["--refresh"]);
        } else {
            configure = configure.args(["--yes"]);
        }
        let output = configure.run().await.unwrap();
        assert_success(&output);
        let output = isolated(executable(), workspace.path())
            .env("NAN_API_KEY", "")
            .args([
                "run",
                "--standalone",
                "--model",
                "nan/qwen3.6#max",
                "--format",
                "json",
                "Reply OK.",
            ])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER), "{}", output.diagnostic());
        assert!(provider.chat_requests().iter().any(|body| {
            body["model"] == "qwen3.6"
                && body["reasoning_effort"] == "max"
                && body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
        }));
        provider.shutdown().await.unwrap();
    }
    let output = isolated(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args(["config", "opencode2", "--remove"])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    let config = workspace.path().join("home/.config/opencode/opencode.json");
    if config.exists() {
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
        assert!(value.pointer("/provider/nan").is_none());
    }
}
