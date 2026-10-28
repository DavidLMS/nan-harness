use super::constants::{INVENTORY_MARKER, TEST_CREDENTIAL, WRAPPER_TIMEOUT};
use super::environment;
use super::helpers::{duration_milliseconds, progress_event};
use super::report::{ConformanceCheck, ConformanceStatus};
use crate::scripted_provider::{ProviderScenario, ScriptedProvider};
use crate::terminal::TerminalCommand;
use crate::workspace::ConformanceWorkspace;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Tests a native Codex launch using a disposable disabled-reasoning preference.
pub async fn codex_reasoning_check(
    nan_harness: &Path,
    model: &str,
    name: &str,
) -> ConformanceCheck {
    let started = Instant::now();
    progress_event(
        "tool-round-trip",
        "native-configuration",
        "started",
        started,
    );
    let passed = verify(nan_harness, model).await.is_ok();
    progress_event(
        "tool-round-trip",
        "native-configuration",
        if passed { "passed" } else { "failed" },
        started,
    );
    ConformanceCheck {
        name: name.to_owned(),
        status: if passed {
            ConformanceStatus::Passed
        } else {
            ConformanceStatus::Failed
        },
        duration_milliseconds: duration_milliseconds(started.elapsed()),
    }
}

async fn verify(nan_harness: &Path, model: &str) -> Result<(), ()> {
    let workspace = ConformanceWorkspace::create().map_err(|_| ())?;
    let root = workspace.path();
    let home = root.join("home");
    let codex_home = home.join(".codex");
    fs::create_dir_all(&codex_home).map_err(|_| ())?;
    let config = "model_reasoning_effort = \"none\"\n";
    fs::write(codex_home.join("config.toml"), config).map_err(|_| ())?;
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .map_err(|_| ())?;
    let mut result = Ok(());
    let mut seen = 0;
    for (selected_model, resume) in [
        (model, false),
        (model, true),
        ("qwen3.6", true),
        (model, true),
    ] {
        let output = run_turn(
            nan_harness,
            root,
            &home,
            &codex_home,
            provider.base_url(),
            selected_model,
            resume,
        )
        .await;
        let requests = provider.chat_requests();
        let recent = requests.get(seen..).ok_or(())?;
        let valid = output.is_ok_and(|output| {
            output.status.success() && output.stdout.contains(INVENTORY_MARKER)
        }) && !recent.is_empty()
            && recent.iter().all(|request| {
                request["model"] == selected_model
                    && if selected_model == "qwen3.6" {
                        request["reasoning_effort"] == "none"
                    } else {
                        request.get("reasoning_effort").is_none()
                            && request.get("chat_template_kwargs").is_none()
                    }
            });
        seen = requests.len();
        if !valid {
            result = Err(());
            break;
        }
    }
    let provider_ok = provider.completed() && provider.recording_bounded();
    let shutdown_ok = provider.shutdown().await.is_ok();
    result?;
    if !provider_ok
        || !shutdown_ok
        || fs::read_to_string(codex_home.join("config.toml"))
            .ok()
            .as_deref()
            != Some(config)
    {
        return Err(());
    }
    Ok(())
}

async fn run_turn(
    nan_harness: &Path,
    root: &Path,
    home: &Path,
    codex_home: &Path,
    provider_url: &str,
    model: &str,
    resume: bool,
) -> Result<crate::terminal::TerminalOutput, ()> {
    let mut arguments = vec![
        "codex",
        "--provider-base-url",
        provider_url,
        "--model",
        model,
        "--",
        "exec",
        "--skip-git-repo-check",
        "--json",
    ];
    if resume {
        arguments.extend(["resume", "--last"]);
    }
    arguments.push("Reply with the marker.");
    let command = TerminalCommand::new(nan_harness, root)
        .clear_environment()
        .args(arguments)
        .env("CI", "1")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("NAN_API_KEY", TEST_CREDENTIAL)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env("NAN_HARNESS_CONFIG_DIR", root.join("nan-config"))
        .env("HOME", home)
        .env("CODEX_HOME", codex_home)
        .timeout(WRAPPER_TIMEOUT);
    #[cfg(windows)]
    let command = environment::apply(command, root).map_err(|_| ())?;
    #[cfg(not(windows))]
    let command = environment::apply(command, root);
    command.run().await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_launcher_fails_the_check_instead_of_skipping_it() {
        let root = tempfile::tempdir().expect("temporary directory");
        let missing = root.path().join("missing-launcher");
        let check = codex_reasoning_check(&missing, "qwen3.8-flash", "reasoning-qwen38").await;
        assert_eq!(check.name, "reasoning-qwen38");
        assert_eq!(check.status, ConformanceStatus::Failed);
    }
}
