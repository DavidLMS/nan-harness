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
fn mimo_auto_search_preserves_external_search_in_every_global_config_format() {
    for custom_home in [false, true] {
        for name in ["config.json", "mimocode.json", "mimocode.jsonc"] {
            let root = tempfile::tempdir().unwrap();
            let state = root.path().join("state");
            let directory = if custom_home {
                root.path().join("mimo-home/config")
            } else {
                root.path().join("xdg-config/mimocode")
            };
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::create_dir(&state).unwrap();
            write_private_credential_fixture(&state, "synthetic-key");
            let path = directory.join(name);
            let original =
                json!({"mcp": {"brave-search": {"type": "local", "command": ["brave-search"]}}});
            std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
            let (endpoint, request) =
                capture_one_http_request_with_response(r#"{"data":[{"id":"qwen3.6"}]}"#);
            let configured = mimo_command(root.path(), &format!("{endpoint}/v1"), custom_home)
                .args(["config", "mimo", "--yes"])
                .output()
                .unwrap();
            assert!(
                configured.status.success(),
                "{}",
                String::from_utf8_lossy(&configured.stderr)
            );
            request.join().unwrap();
            let receipt = read_json(&state.join("configurations.json"));
            assert_eq!(receipt["harnesses"]["mimo-code"]["searchManaged"], false);
            assert!(read_json(&path)["mcp"]["nan-search"].is_null());
            assert_eq!(read_json(&path)["mcp"], original["mcp"]);
            let removed = mimo_command(root.path(), "http://127.0.0.1:1/v1", custom_home)
                .args(["config", "mimo", "--remove"])
                .output()
                .unwrap();
            assert!(removed.status.success());
            assert_eq!(read_json(&path), original);
        }
    }
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
        .env_remove("MIMOCODE_HOME")
        .env_remove("MIMOCODE_CONFIG")
        .env_remove("MIMOCODE_CONFIG_DIR")
        .env_remove("MIMOCODE_CONFIG_CONTENT")
        .current_dir(root);
    if custom_home {
        command.env("MIMOCODE_HOME", root.join("mimo-home"));
    }
    command
}

#[tokio::test]
#[ignore = "requires the pinned MiMo Code executable"]
async fn mimo_runs_directly_with_the_persisted_catalog_and_credential() {
    use nan_harness_test_support::conformance::{
        ConformanceStatus, mimo_native_configuration_check,
    };
    let check = mimo_native_configuration_check(Path::new(env!("CARGO_BIN_EXE_nan-harness"))).await;
    assert_eq!(check.status, ConformanceStatus::Passed, "{check:?}");
}
