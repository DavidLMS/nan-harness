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
    std::fs::create_dir_all(&config).expect("nan-harness config should exist");
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
        .expect("nan-harness should launch fake Codex");
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
        .expect("nan-harness should reject an unavailable explicit model");
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
async fn launch_from_home_uses_a_scoped_profile_and_preserves_user_config() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let home = root.path().join("home");
    let codex_home = root.path().join("codex-home");
    let config = root.path().join("nan-config");
    std::fs::create_dir_all(&home).expect("home should exist");
    std::fs::create_dir_all(&codex_home).expect("Codex home should exist");
    std::fs::create_dir_all(&config).expect("nan-harness config should exist");
    let home_key = serde_json::to_string(home.to_string_lossy().as_ref())
        .expect("home path should serialize as a TOML-compatible string");
    let source_config = format!(
        "notify = [\"notify\", \"turn-ended\"]\n\n[projects.{home_key}]\ntrust_level = \"trusted\"\n"
    );
    std::fs::write(codex_home.join("config.toml"), &source_config)
        .expect("source Codex config should be written");

    let executable = root.path().join("codex");
    std::fs::write(
        &executable,
        concat!(
            "#!/bin/sh\n",
            "if [ \"${1-}\" = \"--version\" ]; then\n",
            "  printf '%s\\n' 'codex-cli 0.148.0'\n",
            "  exit 0\n",
            "fi\n",
            "if [ \"${1-}\" = \"--help\" ]; then\n",
            "  printf '%s\\n' '  -p, --profile <CONFIG_PROFILE_V2>'\n",
            "  exit 0\n",
            "fi\n",
            "test \"$(pwd -P)\" = \"$(cd \"$HOME\" && pwd -P)\"\n",
            "test \"$CODEX_HOME\" = \"$NAN_TEST_ORIGINAL_CODEX_HOME\"\n",
            "grep -Fq 'notify = [\"notify\", \"turn-ended\"]' \"$CODEX_HOME/config.toml\"\n",
            "printf '%s\\n' \"$@\" | grep -Fq 'model=\"qwen3.6\"'\n",
            "printf '%s\\n' \"$@\" | grep -Fq 'model_reasoning_effort=\"high\"'\n",
            "profile=''\n",
            "while [ \"$#\" -gt 0 ]; do\n",
            "  if [ \"$1\" = \"--profile\" ]; then profile=$2; break; fi\n",
            "  shift\n",
            "done\n",
            "test -n \"$profile\"\n",
            "profile_path=$CODEX_HOME/$profile.config.toml\n",
            "grep -Fq 'model = \"qwen3.6\"' \"$profile_path\"\n",
            "grep -Fq 'model_reasoning_effort = \"high\"' \"$profile_path\"\n",
            "printf '%s\\n' 'model = \"mimo-v2.5\"' 'model_reasoning_effort = \"high\"' > \"$profile_path\"\n",
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
        .current_dir(&home)
        .env("NAN_API_KEY", "nan_test_key")
        .env("NAN_HARNESS_CONFIG_DIR", &config)
        .env("NAN_TEST_ORIGINAL_CODEX_HOME", &codex_home)
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env_remove("NAN_COMPATIBILITY_MANIFEST_URL")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .await
        .expect("nan-harness should launch fake Codex from home");
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "unexpected stderr: {stderr}");
    let preserved_config = std::fs::read_to_string(codex_home.join("config.toml"))
        .expect("source Codex config should remain readable");
    assert_eq!(preserved_config, source_config);
    assert!(
        std::fs::read_dir(&codex_home)
            .expect("Codex home should remain readable")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with("nan-harness-launch_"))
    );
    let preferences: serde_json::Value = serde_json::from_slice(
        &std::fs::read(config.join("preferences.json")).expect("preference should exist"),
    )
    .expect("preference should be valid JSON");
    assert_eq!(preferences["lastCodexModel"], "mimo-v2.5");
    assert_eq!(
        preferences["lastCodexReasoning"],
        serde_json::json!({"kind": "toggle", "value": true})
    );
}

#[tokio::test]
#[ignore = "requires the pinned Codex executable"]
async fn codex_native_inventory_crosses_the_responses_bridge() {
    let home = tempfile::tempdir().expect("home should exist");
    let codex_home = home.path().join(".codex");
    let nan_config = tempfile::tempdir().expect("nan-harness config should exist");
    std::fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let source_config = "notify = [\"true\"]\n";
    std::fs::write(codex_home.join("config.toml"), source_config)
        .expect("Codex user config should exist");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let output = TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), home.path())
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
        .env("HOME", home.path())
        .env("CODEX_HOME", &codex_home)
        .env("NAN_HARNESS_CONFIG_DIR", nan_config.path())
        .timeout(Duration::from_secs(90))
        .run()
        .await
        .expect("nan-harness should complete before the timeout");

    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(output.stdout.contains(INVENTORY_MARKER));
    assert!(!output.stdout.contains("NH-BRIDGE-"));
    for stream in [&output.stdout, &output.stderr] {
        assert!(!stream.contains("Project-local config"));
        assert!(!stream.contains("marked as untrusted"));
    }
    assert_eq!(
        std::fs::read_to_string(codex_home.join("config.toml"))
            .expect("Codex user config should remain readable"),
        source_config
    );
    assert!(
        std::fs::read_dir(&codex_home)
            .expect("Codex home should remain readable")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with("nan-harness-launch_"))
    );
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
