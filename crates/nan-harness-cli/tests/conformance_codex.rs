#![cfg(unix)]

use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::TerminalCommand;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const INVENTORY_MARKER: &str = "NAN_HARNESS_CODEX_INVENTORY_OK";

#[tokio::test]
async fn unavailable_saved_model_falls_back_but_explicit_model_stays_strict() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let workspace = root.path().join("workspace");
    let home = root.path().join("home");
    let codex_home = root.path().join("codex-home");
    let config = root.path().join("nan-config");
    std::fs::create_dir_all(&workspace).expect("workspace should exist");
    std::fs::create_dir_all(&home).expect("home should exist");
    std::fs::create_dir_all(&codex_home).expect("Codex home should exist");
    std::fs::create_dir_all(&config).expect("NaN config should exist");
    std::fs::write(
        config.join("preferences.json"),
        r#"{"schemaVersion":1,"lastCodexModel":"retired-model"}"#,
    )
    .expect("stale preference should be written");
    let executable = root.path().join("codex");
    std::fs::write(
        &executable,
        concat!(
            "#!/bin/sh\n",
            "if [ \"${1-}\" = \"--version\" ]; then\n",
            "  printf '%s\\n' 'codex-cli 0.146.0'\n",
            "  exit 0\n",
            "fi\n",
            "grep -Fq 'model = \"qwen3.6\"' \"$CODEX_HOME/config.toml\"\n",
        ),
    )
    .expect("fake Codex should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake Codex should be executable");
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");

    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "codex",
            "--executable",
            executable
                .to_str()
                .expect("executable path should be UTF-8"),
            "--provider-base-url",
            provider.base_url(),
        ])
        .current_dir(&workspace)
        .env("NAN_API_KEY", "nan_test_key")
        .env("NAN_HARNESS_CONFIG_DIR", &config)
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env_remove("NAN_COMPATIBILITY_MANIFEST_URL")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .await
        .expect("NaN should launch fake Codex");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("model 'retired-model' is no longer available"));
    assert!(stderr.contains("using 'qwen3.6'"));
    let preferences: serde_json::Value = serde_json::from_slice(
        &std::fs::read(config.join("preferences.json")).expect("preference should remain"),
    )
    .expect("preference should be valid JSON");
    assert_eq!(preferences["lastCodexModel"], "qwen3.6");

    let explicit = tokio::process::Command::new(env!("CARGO_BIN_EXE_nan"))
        .args([
            "codex",
            "--model",
            "retired-model",
            "--executable",
            executable
                .to_str()
                .expect("executable path should be UTF-8"),
            "--provider-base-url",
            provider.base_url(),
        ])
        .current_dir(&workspace)
        .env("NAN_API_KEY", "nan_test_key")
        .env("NAN_HARNESS_CONFIG_DIR", &config)
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env_remove("NAN_COMPATIBILITY_MANIFEST_URL")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .await
        .expect("NaN should reject an unavailable explicit model");
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
    let explicit_stderr =
        String::from_utf8(explicit.stderr).expect("explicit stderr should be UTF-8");
    assert!(!explicit.status.success());
    assert!(explicit_stderr.contains("model 'retired-model' is not available"));
    assert!(!explicit_stderr.contains("using 'qwen3.6'"));
}

#[tokio::test]
#[ignore = "requires the pinned Codex executable"]
async fn codex_native_inventory_crosses_the_responses_bridge() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let codex_home = tempfile::tempdir().expect("Codex home should exist");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let output = TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args(vec![
            OsString::from("codex"),
            OsString::from("--provider-base-url"),
            OsString::from(provider.base_url()),
            OsString::from("--"),
            OsString::from("exec"),
            OsString::from("--skip-git-repo-check"),
            OsString::from("--ephemeral"),
            OsString::from("--json"),
            OsString::from(format!(
                "Reply exactly {INVENTORY_MARKER} without using tools."
            )),
        ])
        .env("NAN_API_KEY", "nan_test_key")
        .env("CODEX_HOME", codex_home.path())
        .timeout(Duration::from_secs(90))
        .run()
        .await
        .expect("NaN Harness should complete before the timeout");

    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(output.stdout.contains(INVENTORY_MARKER));
    assert!(!output.stdout.contains("NH-BRIDGE-"));
    let requests = provider.chat_requests();
    let tools = requests
        .first()
        .and_then(|request| request.get("tools"))
        .and_then(serde_json::Value::as_array)
        .expect("Codex should advertise tools");
    let tool_names = tools
        .iter()
        .filter_map(|entry| entry.pointer("/function/name"))
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    for tool in ["exec_command", "write_stdin", "apply_patch", "update_plan"] {
        assert!(
            tools.iter().any(|entry| {
                entry
                    .pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(tool)
            }),
            "Codex tool '{tool}' should cross the bridge; received {tool_names:?}"
        );
    }
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}
