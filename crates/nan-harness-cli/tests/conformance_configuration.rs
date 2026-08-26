#![cfg(unix)]

use nan_harness_test_support::conformance::assert_success;
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::{TerminalCommand, TerminalOutput};
use serde_json::json;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DIRECT_MARKER: &str = "NAN_HARNESS_CONFIGURATION_OK";

#[tokio::test]
#[ignore = "requires the pinned Prime Agent executable"]
async fn prime_agent_consumes_the_native_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = ConfigurationEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    configure_provider("prime-agent", provider.base_url(), &environment).await;
    let output = environment
        .command("prime-agent", workspace.path())
        .args(["model", "list", "nan"])
        .run()
        .await
        .expect("Prime Agent should inspect the configured provider");

    assert_success(&output);
    assert!(
        contains_output(&output, "qwen3.6"),
        "{}",
        output.diagnostic()
    );
    assert!(
        contains_output(&output, "gemma4"),
        "{}",
        output.diagnostic()
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned Qwen Code executable"]
async fn qwen_code_consumes_the_native_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = ConfigurationEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::tool(
        "list_directory",
        json!({"path": workspace.path()}),
        DIRECT_MARKER,
    ))
    .await
    .expect("scripted provider should start");

    configure_provider("qwen", provider.base_url(), &environment).await;
    let output = environment
        .command("qwen", workspace.path())
        .args([
            "--model",
            "qwen3.6",
            "--safe-mode",
            "--prompt",
            "Complete the controlled native configuration tool check.",
            "--output-format",
            "json",
        ])
        .run()
        .await
        .expect("Qwen Code should use the configured provider");

    assert_success(&output);
    assert!(
        output.stdout.contains(DIRECT_MARKER),
        "{}",
        output.diagnostic()
    );
    assert!(provider.chat_requests().len() >= 2);
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned DeepSeek Harness executable"]
async fn deepseek_harness_loads_the_native_dynamic_catalog_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = ConfigurationEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    configure_provider("dsh", provider.base_url(), &environment).await;
    let requests_before_direct_launch = provider.chat_requests().len();
    let output = environment
        .command("dsh", workspace.path())
        .env("DSH_TELEMETRY_DISABLED", "1")
        .args([
            "--profile",
            "headless",
            "Reply exactly NAN_HARNESS_CONFIGURATION_OK without using tools.",
        ])
        .run()
        .await
        .expect("DeepSeek Harness should use the configured provider");

    assert_success(&output);
    assert!(
        output.stdout.contains(DIRECT_MARKER),
        "{}",
        output.diagnostic()
    );
    assert!(provider.chat_requests().len() > requests_before_direct_launch);
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned Aider executable"]
async fn aider_consumes_the_native_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = ConfigurationEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    configure_provider("aider", provider.base_url(), &environment).await;
    let output = environment
        .command("aider", workspace.path())
        .args([
            "--model",
            "nan/qwen3.6",
            "--message",
            "Reply exactly NAN_HARNESS_CONFIGURATION_OK.",
            "--yes-always",
            "--no-auto-commits",
            "--no-git",
            "--no-show-model-warnings",
            "--no-check-update",
            "--map-tokens",
            "0",
        ])
        .run()
        .await
        .expect("Aider should use the configured provider");

    assert_success(&output);
    assert!(
        output.stdout.contains(DIRECT_MARKER),
        "{}",
        output.diagnostic()
    );
    assert!(!provider.chat_requests().is_empty());
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

async fn configure_provider(harness: &str, base_url: &str, environment: &ConfigurationEnvironment) {
    let arguments = [
        OsString::from("config"),
        OsString::from(harness),
        OsString::from("--yes"),
    ];
    let output = environment
        .command(env!("CARGO_BIN_EXE_nan-harness"), environment.workspace())
        .env("NAN_BASE_URL", base_url)
        .args(arguments)
        .run()
        .await
        .expect("nan-harness should configure the provider before the timeout");
    assert_success(&output);
}

struct ConfigurationEnvironment {
    workspace: PathBuf,
    home: PathBuf,
}

impl ConfigurationEnvironment {
    fn new(workspace: &Path) -> Self {
        let home = workspace.join("home");
        std::fs::create_dir_all(&home).expect("isolated configuration home should exist");
        let state = home.join(".nan-harness");
        std::fs::create_dir_all(&state).expect("isolated state directory should exist");
        std::fs::write(state.join("nan-api-key"), "nan_test_key")
            .expect("saved credential should be written");
        std::fs::write(
            state.join("credential.json"),
            r#"{"schemaVersion":1,"backend":"private-file"}"#,
        )
        .expect("credential receipt should be written");
        Self {
            workspace: workspace.to_path_buf(),
            home,
        }
    }

    fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn command(&self, program: impl AsRef<OsStr>, current_directory: &Path) -> TerminalCommand {
        TerminalCommand::new(PathBuf::from(program.as_ref()), current_directory)
            .env("HOME", &self.home)
            .env("NAN_HARNESS_CONFIG_DIR", self.home.join(".nan-harness"))
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env(
                "PRIME_AGENT_CODING_AGENT_DIR",
                self.home.join(".prime/agent"),
            )
            .env("QWEN_HOME", self.home.join(".qwen"))
            .env("DSH_HOME", self.home.join(".dsh"))
            .timeout(Duration::from_mins(2))
    }
}

fn contains_output(output: &TerminalOutput, expected: &str) -> bool {
    output.stdout.contains(expected) || output.stderr.contains(expected)
}
