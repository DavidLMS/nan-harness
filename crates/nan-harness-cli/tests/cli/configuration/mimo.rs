use crate::support::{
    capture_one_http_request_with_response, config_command, write_private_credential_fixture,
};
use serde_json::{Value, json};
use std::path::Path;

fn read_json(path: &Path) -> Value {
    jsonc_parser::parse_to_serde_value::<Value>(
        &std::fs::read_to_string(path).unwrap(),
        &jsonc_parser::ParseOptions::default(),
    )
    .unwrap()
}

#[test]
fn mimo_native_configuration_uses_xdg_and_custom_home_and_refreshes_the_saved_key() {
    for custom_home in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let config_home = root.path().join("xdg-config");
        let data_home = root.path().join("xdg-data");
        let mimo_home = root.path().join("mimo-home");
        std::fs::create_dir_all(&state).unwrap();
        write_private_credential_fixture(&state, "first-synthetic-key");
        let response = r#"{"data":[{"id":"qwen3.6"},{"id":"new-model"}]}"#;
        let (endpoint, request) = capture_one_http_request_with_response(response);
        let mut command = mimo_command(root.path(), &format!("{endpoint}/v1"), custom_home);
        let configured = command
            .args(["config", "mimo", "--yes", "--no-search"])
            .output()
            .unwrap();
        assert!(
            configured.status.success(),
            "{}",
            String::from_utf8_lossy(&configured.stderr)
        );
        request.join().unwrap();
        let config = if custom_home {
            mimo_home.join("config/mimocode.jsonc")
        } else {
            config_home.join("mimocode/mimocode.jsonc")
        };
        let auth = if custom_home {
            mimo_home.join("data/auth.json")
        } else {
            data_home.join("mimocode/auth.json")
        };
        assert_eq!(read_json(&config)["model"], "nan/qwen3.6");
        assert!(read_json(&config)["provider"]["nan"]["models"]["new-model"].is_object());
        assert_eq!(read_json(&auth)["nan"]["key"], "first-synthetic-key");
        assert!(
            !std::fs::read_to_string(&config)
                .unwrap()
                .contains("first-synthetic-key")
        );
        write_private_credential_fixture(&state, "rotated-synthetic-key");
        let mut offline = mimo_command(root.path(), "http://127.0.0.1:1/v1", custom_home);
        let status = offline
            .args(["config", "mimo-code", "--status"])
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(String::from_utf8_lossy(&status.stdout).contains("copied key needs"));
        let (endpoint, request) =
            capture_one_http_request_with_response(r#"{"data":[{"id":"replacement"}]}"#);
        let mut refresh = mimo_command(root.path(), &format!("{endpoint}/v1"), custom_home);
        assert!(
            refresh
                .args(["config", "mimo", "--refresh"])
                .output()
                .unwrap()
                .status
                .success()
        );
        request.join().unwrap();
        assert_eq!(read_json(&auth)["nan"]["key"], "rotated-synthetic-key");
        assert_eq!(read_json(&config)["model"], "nan/replacement");
        let mut remove = mimo_command(root.path(), "http://127.0.0.1:1/v1", custom_home);
        assert!(
            remove
                .args(["config", "mimo", "--remove"])
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(!auth.exists() && !config.exists());
        let receipt = std::fs::read_to_string(state.join("configurations.json")).unwrap();
        assert!(!receipt.contains("rotated-synthetic-key"));
    }
}

#[cfg(unix)]
#[test]
fn uninstall_removes_mimo_configuration_and_preserves_user_preferences() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let state = root.path().join("state");
    let mimo_home = root.path().join("mimo-home");
    let config = mimo_home.join("config/mimocode.jsonc");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(&config, "{ /* user preferences */ \"theme\": \"user\" }\n").unwrap();
    write_private_credential_fixture(&state, "synthetic-key");
    let (endpoint, request) =
        capture_one_http_request_with_response(r#"{"data":[{"id":"qwen3.6"}]}"#);
    let result = config_command(&home, &state, &format!("{endpoint}/v1"))
        .env("MIMOCODE_HOME", &mimo_home)
        .args(["config", "mimo", "--yes", "--no-search"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    request.join().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("nan-harness");
    std::fs::copy(env!("CARGO_BIN_EXE_nan-harness"), &executable).unwrap();
    std::fs::write(
        state.join("installation.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 1, "executablePath": executable,
            "aliasPath": bin.join("nanh"), "userPathEntryAdded": false,
        }))
        .unwrap(),
    )
    .unwrap();
    let result = std::process::Command::new(&executable)
        .args(["uninstall", "--yes"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("HERMES_HOME", home.join(".hermes"))
        .env("MIMOCODE_HOME", &mimo_home)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(read_json(&config), json!({"theme": "user"}));
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("/* user preferences */")
    );
    assert!(!mimo_home.join("data/auth.json").exists());
}

fn mimo_command(root: &Path, base_url: &str, custom_home: bool) -> std::process::Command {
    let home = root.join("home");
    let mut command = config_command(&home, &root.join("state"), base_url);
    command
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env_remove("MIMOCODE_HOME");
    if custom_home {
        command.env("MIMOCODE_HOME", root.join("mimo-home"));
    }
    command
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires the pinned MiMo Code executable"]
async fn mimo_runs_directly_with_the_persisted_catalog_and_credential() {
    use nan_harness_test_support::conformance::{TEST_CREDENTIAL, assert_success, call};
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
    use nan_harness_test_support::terminal::TerminalCommand;
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    write_private_credential_fixture(&state, TEST_CREDENTIAL);
    let target = root.path().join("read-target.txt");
    std::fs::write(&target, "MIMO_PERSISTENT_READ_OK\n").unwrap();
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        [call("read", json!({"file_path": target}))],
        "MIMO_PERSISTENT_OK",
    ))
    .await
    .unwrap();
    let mut configure = mimo_command(root.path(), provider.base_url(), true);
    configure.args(["config", "mimo", "--yes", "--no-search"]);
    let configured = tokio::process::Command::from(configure)
        .output()
        .await
        .unwrap();
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );
    let output = TerminalCommand::new("mimo", root.path())
        .clear_environment()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root.path().join("home"))
        .env("MIMOCODE_HOME", root.path().join("mimo-home"))
        .env("CI", "1")
        .args([
            "run",
            "--pure",
            "--format",
            "json",
            "--dangerously-skip-permissions",
            "Read the fixture and complete the deterministic check.",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .run()
        .await
        .unwrap();
    assert_success(&output);
    assert!(
        output.stdout.contains("MIMO_PERSISTENT_OK"),
        "{}",
        output.diagnostic()
    );
    assert!(provider.completed());
    let requests = provider.chat_requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|request| request["model"] == "qwen3.6"));
    provider.shutdown().await.unwrap();
}
