use crate::support::{
    capture_one_http_request_with_response, config_command, write_private_credential_fixture,
};
use std::path::Path;

const USER_CONFIG: &str = "# my preferences\nactive_model = \"mistral-medium-3.5\"\n\n[[mcp_servers]]\nname = \"user-docs\"\ntransport = \"stdio\"\ncommand = \"user-docs-server\"\n";
const USER_ENV: &str = "# my keys\nMISTRAL_API_KEY=\"user-secret\"\n";

fn vibe_command(root: &Path, base_url: &str, vibe_home: Option<&Path>) -> std::process::Command {
    let home = root.join("home");
    let mut command = config_command(&home, &root.join("state"), base_url);
    command
        .env("USERPROFILE", &home)
        .env_remove("VIBE_HOME")
        .current_dir(root);
    if let Some(vibe_home) = vibe_home {
        command.env("VIBE_HOME", vibe_home);
    }
    command
}

#[test]
fn vibe_native_configuration_honors_vibe_home_rotates_the_saved_key_and_restores_files() {
    for custom_home in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let custom = root.path().join("vibe-home");
        let (vibe_home, selected) = if custom_home {
            (custom.clone(), Some(custom.as_path()))
        } else {
            (root.path().join("home/.vibe"), None)
        };
        std::fs::create_dir_all(&vibe_home).unwrap();
        std::fs::write(vibe_home.join("config.toml"), USER_CONFIG).unwrap();
        std::fs::write(vibe_home.join(".env"), USER_ENV).unwrap();
        write_private_credential_fixture(&state, "first-synthetic-key");
        let (endpoint, request) = capture_one_http_request_with_response(
            r#"{"data":[{"id":"qwen3.6"},{"id":"new-model"}]}"#,
        );
        let configured = vibe_command(root.path(), &format!("{endpoint}/v1"), selected)
            .args(["config", "vibe", "--yes", "--no-search"])
            .output()
            .unwrap();
        assert!(
            configured.status.success(),
            "{}",
            String::from_utf8_lossy(&configured.stderr)
        );
        request.join().unwrap();
        let config = std::fs::read_to_string(vibe_home.join("config.toml")).unwrap();
        assert!(config.contains("active_model = \"qwen3.6\""));
        assert!(config.contains("name = \"new-model\""));
        assert!(config.contains("user-docs-server"));
        assert!(!config.contains("first-synthetic-key"));
        let env = std::fs::read_to_string(vibe_home.join(".env")).unwrap();
        assert!(env.starts_with(USER_ENV));
        assert!(env.contains("NAN_VIBE_API_KEY=\"first-synthetic-key\""));

        write_private_credential_fixture(&state, "rotated-synthetic-key");
        let status = vibe_command(root.path(), "http://127.0.0.1:1/v1", selected)
            .args(["config", "mistral-vibe", "--status"])
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(String::from_utf8_lossy(&status.stdout).contains("copied key needs"));
        let (endpoint, request) =
            capture_one_http_request_with_response(r#"{"data":[{"id":"replacement"}]}"#);
        let refreshed = vibe_command(root.path(), &format!("{endpoint}/v1"), selected)
            .args(["config", "vibe", "--refresh"])
            .output()
            .unwrap();
        assert!(refreshed.status.success());
        request.join().unwrap();
        let env = std::fs::read_to_string(vibe_home.join(".env")).unwrap();
        assert!(env.contains("NAN_VIBE_API_KEY=\"rotated-synthetic-key\""));
        assert!(!env.contains("first-synthetic-key"));
        assert!(
            std::fs::read_to_string(vibe_home.join("config.toml"))
                .unwrap()
                .contains("active_model = \"replacement\"")
        );

        let removed = vibe_command(root.path(), "http://127.0.0.1:1/v1", selected)
            .args(["config", "vibe", "--remove"])
            .output()
            .unwrap();
        assert!(removed.status.success());
        assert_eq!(
            std::fs::read_to_string(vibe_home.join("config.toml")).unwrap(),
            USER_CONFIG
        );
        assert_eq!(
            std::fs::read_to_string(vibe_home.join(".env")).unwrap(),
            USER_ENV
        );
        let receipt = std::fs::read_to_string(state.join("configurations.json")).unwrap();
        assert!(!receipt.contains("synthetic-key"));
    }
}

#[tokio::test]
#[ignore = "requires the pinned Mistral Vibe executable"]
async fn vibe_runs_directly_with_the_persisted_catalog_and_credential() {
    use nan_harness_test_support::conformance::{TEST_CREDENTIAL, assert_success, call};
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
    use nan_harness_test_support::terminal::TerminalCommand;
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    write_private_credential_fixture(&state, TEST_CREDENTIAL);
    let target = root.path().join("read-target.txt");
    std::fs::write(&target, "VIBE_PERSISTENT_READ_OK\n").unwrap();
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        [call("read_file", json!({"path": target}))],
        "VIBE_PERSISTENT_OK",
    ))
    .await
    .unwrap();
    let vibe_home = root.path().join("vibe-home");
    let configured = tokio::process::Command::from({
        let mut configure = vibe_command(root.path(), provider.base_url(), Some(&vibe_home));
        configure.args(["config", "vibe", "--yes", "--no-search"]);
        configure
    })
    .output()
    .await
    .unwrap();
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );
    let output = TerminalCommand::new("vibe", root.path())
        .clear_environment()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root.path().join("home"))
        .env("VIBE_HOME", &vibe_home)
        .env("CI", "1")
        .args([
            "--trust",
            "--auto-approve",
            "--output",
            "text",
            "--prompt",
            "Read the fixture and complete the deterministic check.",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .run()
        .await
        .unwrap();
    assert_success(&output);
    assert!(
        output.stdout.contains("VIBE_PERSISTENT_OK"),
        "{}",
        output.diagnostic()
    );
    assert!(provider.completed());
    let requests = provider.chat_requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|request| request["model"] == "qwen3.6"));
    provider.shutdown().await.unwrap();
}
