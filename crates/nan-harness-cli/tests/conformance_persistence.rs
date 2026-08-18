#![cfg(unix)]

use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::{TerminalCommand, TerminalOutput};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DIRECT_MARKER: &str = "NAN_HARNESS_PERSISTENCE_OK";

#[tokio::test]
#[ignore = "requires the pinned Prime Agent executable"]
async fn prime_agent_consumes_the_persisted_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = PersistenceEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    persist_provider("prime", provider.base_url(), &environment, ["--version"]).await;
    let output = environment
        .command("prime-agent", workspace.path())
        .args(["model", "list", "nan"])
        .run()
        .await
        .expect("Prime Agent should inspect the persisted provider");

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
async fn qwen_code_consumes_the_persisted_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = PersistenceEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    persist_provider("qwen", provider.base_url(), &environment, ["--version"]).await;
    let output = environment
        .command("qwen", workspace.path())
        .args([
            "--model",
            "qwen3.6",
            "--safe-mode",
            "--prompt",
            "Reply exactly NAN_HARNESS_PERSISTENCE_OK without using tools.",
            "--output-format",
            "json",
        ])
        .run()
        .await
        .expect("Qwen Code should use the persisted provider");

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

#[tokio::test]
#[ignore = "requires the pinned DeepSeek Harness executable"]
async fn deepseek_harness_loads_the_persisted_dynamic_catalog_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = PersistenceEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    persist_provider(
        "deepseek",
        provider.base_url(),
        &environment,
        [
            "--profile",
            "headless",
            "Reply exactly NAN_HARNESS_PERSISTENCE_OK without using tools.",
        ],
    )
    .await;
    let requests_before_direct_launch = provider.chat_requests().len();
    let settings_path = environment.home.join(".dsh/settings.yaml");
    let mut settings = std::fs::read_to_string(&settings_path)
        .expect("persisted DeepSeek settings should be readable");
    settings.push_str("\nagent-default-model:\n  provider: nan-harness\n  model: qwen3.6\n");
    std::fs::write(&settings_path, settings)
        .expect("the user-owned default model selection should be written");
    let output = environment
        .command("dsh", workspace.path())
        .env("DSH_TELEMETRY_DISABLED", "1")
        .args([
            "--profile",
            "headless",
            "Reply exactly NAN_HARNESS_PERSISTENCE_OK without using tools.",
        ])
        .run()
        .await
        .expect("DeepSeek Harness should use the persisted provider");

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
async fn aider_consumes_the_persisted_dynamic_provider_directly() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let environment = PersistenceEnvironment::new(workspace.path());
    let provider = ScriptedProvider::start(ProviderScenario::inventory(DIRECT_MARKER))
        .await
        .expect("scripted provider should start");

    persist_provider("aider", provider.base_url(), &environment, ["--version"]).await;
    let output = environment
        .command("aider", workspace.path())
        .args([
            "--model",
            "nan/qwen3.6",
            "--message",
            "Reply exactly NAN_HARNESS_PERSISTENCE_OK.",
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
        .expect("Aider should use the persisted provider");

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

async fn persist_provider<const N: usize>(
    harness: &str,
    base_url: &str,
    environment: &PersistenceEnvironment,
    harness_arguments: [&str; N],
) {
    let mut arguments = vec![
        OsString::from(harness),
        OsString::from("--provider-base-url"),
        OsString::from(base_url),
        OsString::from("--persist"),
        OsString::from("--"),
    ];
    arguments.extend(harness_arguments.into_iter().map(OsString::from));
    let output = environment
        .command(env!("CARGO_BIN_EXE_nan-harness"), environment.workspace())
        .args(arguments)
        .run()
        .await
        .expect("NaN should persist the provider before the timeout");
    assert_success(&output);
}

struct PersistenceEnvironment {
    workspace: PathBuf,
    home: PathBuf,
}

impl PersistenceEnvironment {
    fn new(workspace: &Path) -> Self {
        let home = workspace.join("home");
        std::fs::create_dir_all(&home).expect("isolated persistence home should exist");
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
            .env("NAN_API_KEY", "nan_test_key")
            .env("NAN_HARNESS_CONFIG_DIR", self.home.join(".nan-harness"))
            .env(
                "PRIME_AGENT_CODING_AGENT_DIR",
                self.home.join(".prime/agent"),
            )
            .env("QWEN_HOME", self.home.join(".qwen"))
            .env("DSH_HOME", self.home.join(".dsh"))
            .timeout(Duration::from_mins(2))
    }
}

fn assert_success(output: &TerminalOutput) {
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(!output.stdout.contains("NH-"), "{}", output.diagnostic());
}

fn contains_output(output: &TerminalOutput, expected: &str) -> bool {
    output.stdout.contains(expected) || output.stderr.contains(expected)
}
