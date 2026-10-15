//! Isolated native V2 server ownership; never discovers or stops the user's service.
use nan_harness_test_support::conformance::assert_success;
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use nan_harness_test_support::terminal::{CaptureMode, ProcessEvent, ScanState, TerminalCommand};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn executable() -> PathBuf {
    std::env::var_os("NAN_OPENCODE_V2_EXECUTABLE")
        .map_or_else(|| PathBuf::from("opencode2"), PathBuf::from)
}

fn environment(workspace: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    let home = workspace.join("home");
    std::fs::create_dir_all(home.join("tmp")).unwrap();
    let mut values = vec![
        ("PATH", std::env::var_os("PATH").unwrap_or_default()),
        ("HOME", home.clone().into_os_string()),
        ("USERPROFILE", home.clone().into_os_string()),
        ("TEMP", home.join("tmp").into_os_string()),
        ("TMP", home.join("tmp").into_os_string()),
        ("OPENCODE_TEST_HOME", home.clone().into_os_string()),
        ("XDG_CONFIG_HOME", home.join(".config").into_os_string()),
        ("XDG_DATA_HOME", home.join(".local/share").into_os_string()),
        ("XDG_STATE_HOME", home.join(".local/state").into_os_string()),
        ("XDG_CACHE_HOME", home.join(".cache").into_os_string()),
        (
            "NAN_HARNESS_CONFIG_DIR",
            home.join(".nan-harness").into_os_string(),
        ),
        ("NAN_HARNESS_CREDENTIAL_BACKEND", "file".into()),
        ("NAN_API_KEY", "synthetic-lifecycle-key".into()),
    ];
    for name in [
        "NAN_NO_UPDATE_CHECK",
        "NAN_NO_COMPATIBILITY_CHECK",
        "OPENCODE_DISABLE_MODELS_FETCH",
        "OPENCODE_DISABLE_AUTOUPDATE",
        "NO_COLOR",
        "CI",
    ] {
        values.push((name, "1".into()));
    }
    for name in ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(name) {
            values.push((name, value));
        }
    }
    values
}

fn isolated(program: impl Into<PathBuf>, workspace: &Path) -> TerminalCommand {
    let mut command = TerminalCommand::new(program, workspace)
        .clear_environment()
        .timeout(Duration::from_mins(1));
    for (name, value) in environment(workspace) {
        command = command.env(name, value);
    }
    command
}

async fn start_shared_service(workspace: &Path) -> std::io::Result<std::process::Output> {
    // The fixture service intentionally outlives this starter. TerminalCommand's
    // Windows job would kill it on starter exit; the test stops it explicitly.
    let mut command = tokio::process::Command::new(executable());
    command
        .current_dir(workspace)
        .env_clear()
        .envs(environment(workspace))
        .args(["service", "start"])
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_mins(1), command.output())
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "service startup timed out")
        })?
}

fn managed(workspace: &Path, base_url: &str) -> TerminalCommand {
    isolated(env!("CARGO_BIN_EXE_nan-harness"), workspace)
        .args(["opencode2", "--model", "qwen3.6", "--executable"])
        .args([executable()])
        .args([
            "--provider-base-url",
            base_url,
            "--no-search",
            "--no-chat-gateway",
        ])
        .args([
            "--",
            "run",
            "--format",
            "json",
            "Reply with the fixture marker.",
        ])
        .observe_processes(true)
}

#[tokio::test]
#[ignore = "requires OpenCode V2; NAN_OPENCODE_V2_EXECUTABLE selects the binary"]
async fn concurrent_private_servers_exit_without_changing_the_shared_service() {
    let workspace = tempfile::tempdir().unwrap();
    let first = ScriptedProvider::start(
        ProviderScenario::inventory("NAN_PRIVATE_FIRST").with_api_key("synthetic-lifecycle-key"),
    )
    .await
    .unwrap();
    let second = ScriptedProvider::start(
        ProviderScenario::inventory("NAN_PRIVATE_SECOND").with_api_key("synthetic-lifecycle-key"),
    )
    .await
    .unwrap();
    let started = start_shared_service(workspace.path()).await;

    // Defer every assertion until after stopping our isolated shared service, including errors
    // returned by either managed invocation. All subprocesses have finite ownership budgets.
    let before = isolated(executable(), workspace.path())
        .args(["service", "status"])
        .run()
        .await;
    let (first_run, second_run) = tokio::join!(
        managed(workspace.path(), first.base_url()).diagnose(CaptureMode::Pipe, None),
        managed(workspace.path(), second.base_url()).diagnose(CaptureMode::Pipe, None),
    );
    let after = isolated(executable(), workspace.path())
        .args(["service", "status"])
        .run()
        .await;
    let stopped = isolated(executable(), workspace.path())
        .args(["service", "stop"])
        .run()
        .await;
    first.shutdown().await.unwrap();
    second.shutdown().await.unwrap();

    let started = started.unwrap();
    assert!(started.status.success(), "{started:?}");
    let before = before.unwrap();
    let after = after.unwrap();
    assert_success(&before);
    assert_success(&after);
    assert_success(&stopped.unwrap());
    assert_eq!(
        before.stdout.trim(),
        after.stdout.trim(),
        "shared endpoint changed"
    );
    assert_eq!(
        String::from_utf8_lossy(&started.stdout).trim(),
        after.stdout.trim()
    );
    for (run, marker) in [
        (first_run, "NAN_PRIVATE_FIRST"),
        (second_run, "NAN_PRIVATE_SECOND"),
    ] {
        assert_eq!(run.event, ProcessEvent::Exited, "{run:?}");
        assert!(run.status.is_some_and(|status| status.success()), "{run:?}");
        assert!(run.stdout.contains(marker), "{run:?}");
        assert_eq!(
            run.cleanup, None,
            "test harness had to terminate descendants: {run:?}"
        );
        assert_eq!(
            run.survivors_residual.state,
            ScanState::Available,
            "{run:?}"
        );
        assert_eq!(
            run.survivors_residual.count, 0,
            "private server survived exit: {run:?}"
        );
    }
}

#[tokio::test]
#[ignore = "requires OpenCode V2; NAN_OPENCODE_V2_EXECUTABLE selects the binary"]
async fn native_catalog_variant_transmits_reasoning_effort() {
    use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, SupportedReasoningEfforts};
    let workspace = tempfile::tempdir().unwrap();
    let provider = ScriptedProvider::start(
        ProviderScenario::inventory("NAN_NATIVE_VARIANT").with_api_key("synthetic-lifecycle-key"),
    )
    .await
    .unwrap();
    let mut model = nan_harness_core::CodingModelProfile::generic("native-fixture");
    model.reasoning = ReasoningPolicy::Effort {
        supported: SupportedReasoningEfforts::ALL,
        default: ReasoningEffort::High,
        supports_disabled: true,
    };
    let config = serde_json::json!({"providers": {"nan": {
        "package": "@opencode/ai/providers/openai-compatible",
        "settings": {"apiKey": "{env:NAN_API_KEY}", "baseURL": provider.base_url()},
        "models": nan_harness_runtime::opencode_v2_model_catalog(&[model]),
    }}});
    let output = isolated(executable(), workspace.path())
        .env("OPENCODE_CONFIG_CONTENT", config.to_string())
        .args([
            "run",
            "--standalone",
            "--model",
            "nan/native-fixture#max",
            "--format",
            "json",
            "Reply OK.",
        ])
        .run()
        .await;
    let requests = provider.chat_requests();
    provider.shutdown().await.unwrap();
    let output = output.unwrap();
    assert_success(&output);
    assert!(
        output.stdout.contains("NAN_NATIVE_VARIANT"),
        "{}",
        output.diagnostic()
    );
    assert!(
        requests
            .iter()
            .any(|body| body["model"] == "native-fixture" && body["reasoning_effort"] == "max")
    );
}
