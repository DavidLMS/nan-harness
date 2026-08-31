use crate::app::HermesDesktopArgs;
use crate::commands::credentials;
use crate::commands::install::check_required_runtime;
use crate::commands::persistence::{
    PersistenceManager, config_directory, discover_models, write_private_file,
};
use crate::error::CliError;
use crate::runner::discover_or_install_harness;
use nan_harness_adapters::{hermes_search_provider_files, render_hermes_desktop_provider_block};
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, HarnessKind,
};
use nan_harness_private_fs::{PrivatePathKind, open_private_new, restrict_path};
use nan_harness_runtime::{
    BridgeDiagnostic, ChatGatewayError, DesktopCompatibilityEvidence, DesktopCompatibilityStatus,
    RunningChatCompletionsGateway, classify_desktop_version, desktop_compatibility,
    start_chat_completions_gateway,
};
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::process::{Child, Command as TokioCommand};

const PROFILE_NAME: &str = "nan";
const DIAGNOSTIC_PROFILE_PREFIX: &str = "nan-diagnostic-";
const PARKED_PROFILES_DIRECTORY: &str = ".nan-harness";
const RECOVERED_PROFILES_DIRECTORY: &str = "recovered";
const OWNERSHIP_SCHEMA_VERSION: u8 = 1;
const SESSION_SCHEMA_VERSION: u8 = 1;
const OWNER_MARKER_FILE: &str = ".nan-harness-owner.json";
const ENV_BLOCK_BEGIN: &str = "# nan-harness:begin hermes-desktop-session";
const ENV_BLOCK_END: &str = "# nan-harness:end hermes-desktop-session";
const DEFAULT_MODEL_ID: &str = "qwen3.6";
const UPDATE_WAIT_TIMEOUT: Duration = Duration::from_mins(20);
const UPDATE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const RELAUNCH_WAIT_TIMEOUT: Duration = Duration::from_mins(1);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DESKTOP_QUIESCENCE_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) async fn run(
    arguments: &HermesDesktopArgs,
    interactive: bool,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    validate_arguments(arguments)?;
    if arguments.no_chat_gateway && !arguments.run.dry_run && !arguments.restore {
        eprintln!(
            "warning: Chat Completions gateway disabled; provider usage and gateway-dependent search are unavailable"
        );
    }
    let paths = DesktopPaths::from_environment()?;

    if arguments.restore {
        let _lock = SessionLock::acquire(&paths)?;
        ensure_recovery_is_safe(&paths)?;
        restore_session(&paths)?;
        quarantine_recreated_profile_for_restore(&paths)?;
        park_managed_profile_if_owned(&paths)?;
        cleanup_stale_diagnostic_profiles(&paths)?;
        eprintln!("Hermes Desktop managed launch state restored and its NaN profile parked.");
        return Ok(0);
    }

    if arguments.run.dry_run {
        return print_dry_run(arguments, working_directory, &paths);
    }

    run_desktop_session(
        arguments,
        interactive,
        working_directory,
        bridge_diagnostics,
        &paths,
    )
    .await
}

pub(crate) fn persistent_profile_exists() -> Result<bool, HermesDesktopError> {
    let paths = DesktopPaths::from_environment()?;
    Ok(persistent_profile_exists_at(&paths))
}

pub(crate) fn remove_persistent_profile() -> Result<bool, HermesDesktopError> {
    let paths = DesktopPaths::from_environment()?;
    remove_persistent_profile_at(&paths, running_desktop)
}

fn persistent_profile_exists_at(paths: &DesktopPaths) -> bool {
    paths.ownership_receipt.exists()
        || paths.managed_profile.exists()
        || paths.parked_profile.exists()
}

fn remove_persistent_profile_at(
    paths: &DesktopPaths,
    running_desktop: impl FnOnce() -> Result<Option<DesktopProcess>, HermesDesktopError>,
) -> Result<bool, HermesDesktopError> {
    if !paths.session_receipt.exists() && !persistent_profile_exists_at(paths) {
        return Ok(false);
    }
    let _lock = SessionLock::acquire(paths)?;
    if paths.session_receipt.exists() {
        return Err(HermesDesktopError::PendingRecovery);
    }
    if !persistent_profile_exists_at(paths) {
        return Ok(false);
    }
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning);
    }
    park_managed_profile_if_owned(paths)?;
    let Some(ownership) = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)? else {
        return Ok(false);
    };
    let marker = read_optional_json::<OwnerMarker>(&paths.parked_profile.join(OWNER_MARKER_FILE))?
        .ok_or(HermesDesktopError::ManagedProfileMissing)?;
    validate_ownership(&ownership, &marker)?;
    fs::remove_dir_all(&paths.parked_profile).map_err(HermesDesktopError::RemoveProfile)?;
    remove_if_exists(&paths.ownership_receipt).map_err(HermesDesktopError::RemoveReceipt)?;
    remove_profile_guard(paths)?;
    reset_managed_active_profile(paths)?;
    Ok(true)
}

fn print_dry_run(
    arguments: &HermesDesktopArgs,
    _working_directory: &Path,
    _paths: &DesktopPaths,
) -> Result<i32, CliError> {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Hermes,
        if arguments.no_chat_gateway {
            DesktopTransport::DirectChatCompletions
        } else {
            DesktopTransport::ChatCompletionsGateway
        },
    );
    plan.executable.clone_from(&arguments.run.executable);
    plan.selected_model.clone_from(&arguments.run.model);
    plan.web_search_policy = crate::runner::web_search_policy(&arguments.run);
    plan.persistent_profile = !arguments.no_chat_gateway;
    plan.native_arguments.clone_from(&arguments.run.arguments);
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(HermesDesktopError::Serialize)?
    );
    Ok(0)
}

#[allow(clippy::too_many_lines)]
async fn run_desktop_session(
    arguments: &HermesDesktopArgs,
    interactive: bool,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
    paths: &DesktopPaths,
) -> Result<i32, CliError> {
    let _lock = SessionLock::acquire(paths)?;
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning.into());
    }
    if live_update_owner(&paths.update_marker)?.is_some() {
        return Err(HermesDesktopError::UpdateAlreadyRunning.into());
    }
    if paths.session_receipt.exists() {
        restore_session(paths)?;
    }
    park_managed_profile_if_owned(paths)?;
    cleanup_stale_diagnostic_profiles(paths)?;

    let Some(discovery) = discover_or_install_harness(HarnessKind::Hermes, &arguments.run)? else {
        return Ok(0);
    };
    validate_desktop_compatibility(
        &discovery.harness.executable,
        &discovery.harness.detected_version,
        arguments.run.allow_unsupported,
        arguments.run.allow_untested,
    )?;
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let launch_arguments = desktop_arguments(paths, &arguments.run.arguments);

    check_required_runtime(HarnessKind::Hermes)?;
    let mut config =
        credentials::resolve_or_onboard(arguments.run.provider_base_url.clone(), interactive)
            .await?;
    let models = if let Some(models) = config.model_catalog.take() {
        models
    } else {
        discover_models(&config.config).await?
    };
    let manager = PersistenceManager::from_environment()?;
    let remembered_model = if arguments.run.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Hermes)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let selected_model = select_model(
        &models,
        arguments
            .run
            .model
            .as_deref()
            .or(remembered_model.as_deref()),
    )?;
    let mut gateway = prepare_profile_session(
        arguments.no_chat_gateway,
        paths,
        &config.config,
        &models,
        selected_model,
        !arguments.run.search.no_search,
    )
    .await?;

    let marker_before_launch = marker_fingerprint(&paths.update_marker);
    let mut child = match spawn_desktop(
        &discovery.harness.executable,
        &launch_arguments,
        paths,
        working_directory,
    ) {
        Ok(child) => child,
        Err(error) => {
            restore_session(paths)?;
            if let Some(running) = gateway.take() {
                let _ = running.shutdown().await;
            }
            park_managed_profile_if_owned(paths)?;
            return Err(error.into());
        }
    };
    eprintln!(
        "Hermes Desktop launched through NaN profile '{PROFILE_NAME}' with model '{selected_model}'."
    );

    let mut signals = termination_signals();
    let lifecycle = supervise_desktop(
        &mut child,
        gateway.as_mut(),
        paths,
        marker_before_launch,
        &mut signals,
    )
    .await;
    let (exit_code, usage) =
        finish_desktop_session(lifecycle, gateway, paths, bridge_diagnostics).await?;
    if exit_code == 0
        && let Err(error) =
            manager.save_last_desktop_selection(DesktopHarnessKind::Hermes, selected_model)
    {
        eprintln!("warning: could not save the last Desktop model: {error}");
    }
    if let Some(usage) = usage {
        let outcome = if exit_code == 0 {
            nan_harness_runtime::ExecutionOutcome::Succeeded
        } else {
            nan_harness_runtime::ExecutionOutcome::Failed
        };
        if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
            eprintln!("{summary}");
        }
    }
    Ok(exit_code)
}

async fn prepare_profile_session(
    no_chat_gateway: bool,
    paths: &DesktopPaths,
    config: &nan_harness_runtime::ResolvedConfig,
    models: &[CodingModelProfile],
    selected_model: &str,
    web_search_enabled: bool,
) -> Result<Option<RunningChatCompletionsGateway>, HermesDesktopError> {
    if no_chat_gateway {
        let profile = create_diagnostic_profile(paths)?;
        write_profile_config(
            &profile,
            &config.provider_base_url,
            models,
            selected_model,
            false,
        )?;
        config
            .secrets
            .with_secret(&config.provider_credential_ref, |provider_key| {
                begin_session(paths, &profile, SessionMode::Diagnostic, provider_key)
            })
            .map_err(HermesDesktopError::Secret)??;
        return Ok(None);
    }

    let mut ownership = ensure_managed_profile(paths)?;
    activate_managed_profile(paths, &ownership)?;
    remove_legacy_profile_display_name(&paths.managed_profile)?;
    let result = async {
        let listener = bind_stable_gateway(paths, &mut ownership).await?;
        let running =
            start_chat_completions_gateway(config, listener, selected_model, web_search_enabled)
                .map_err(HermesDesktopError::Gateway)?;
        if let Err(error) = write_profile_config(
            &paths.managed_profile,
            &running.client_base_url(),
            models,
            selected_model,
            web_search_enabled,
        ) {
            let _ = running.shutdown().await;
            return Err(error);
        }
        let setup = running.with_session_token(|token| {
            begin_session(
                paths,
                &paths.managed_profile,
                SessionMode::Persistent,
                token,
            )
        });
        if let Err(error) = setup {
            let _ = running.shutdown().await;
            return Err(error);
        }
        Ok(running)
    }
    .await;
    match result {
        Ok(gateway) => Ok(Some(gateway)),
        Err(error) => {
            restore_session(paths)?;
            park_managed_profile_if_owned(paths)?;
            Err(error)
        }
    }
}

async fn finish_desktop_session(
    lifecycle: Result<LifecycleCompletion, HermesDesktopError>,
    mut gateway: Option<RunningChatCompletionsGateway>,
    paths: &DesktopPaths,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<(i32, Option<nan_harness_runtime::ProviderUsageSnapshot>), HermesDesktopError> {
    let persistent_profile = gateway.is_some();
    let preserve_recovery = matches!(
        lifecycle,
        Ok(LifecycleCompletion::PreserveRecovery(_))
            | Err(HermesDesktopError::UpdateTimedOut | HermesDesktopError::UpdateStillRunning)
    ) || live_update_owner(&paths.update_marker)?.is_some();
    if !preserve_recovery {
        if running_desktop()?.is_some() {
            terminate_desktop().await?;
        }
        restore_session(paths)?;
        if persistent_profile {
            park_managed_profile(paths)?;
        }
    }

    let shutdown_result = if let Some(running) = gateway.take() {
        running.shutdown_with_usage().await.map(Some)
    } else {
        Ok(None)
    };
    let usage = match shutdown_result {
        Ok(Some((diagnostics, usage))) => {
            append_diagnostics(bridge_diagnostics, diagnostics);
            Some(usage)
        }
        Err(error) if lifecycle.is_ok() => return Err(HermesDesktopError::Gateway(error)),
        Ok(None) | Err(_) => None,
    };

    let exit_code = match lifecycle? {
        LifecycleCompletion::Closed(exit_code)
        | LifecycleCompletion::PreserveRecovery(exit_code) => exit_code,
    };
    Ok((exit_code, usage))
}

fn validate_arguments(arguments: &HermesDesktopArgs) -> Result<(), HermesDesktopError> {
    if arguments.restore
        && (arguments.run.model.is_some()
            || arguments.run.executable.is_some()
            || arguments.run.provider_base_url.is_some()
            || arguments.run.allow_unsupported
            || arguments.run.allow_untested
            || arguments.run.dry_run
            || arguments.no_chat_gateway
            || !arguments.run.arguments.is_empty())
    {
        return Err(HermesDesktopError::RestoreWithLaunchOptions);
    }
    if let Some(unsupported) = unsupported_desktop_argument(&arguments.run.arguments) {
        return Err(HermesDesktopError::UnsupportedDesktopArgument(unsupported));
    }
    Ok(())
}

fn unsupported_desktop_argument(arguments: &[String]) -> Option<&'static str> {
    ["--build-only", "--setup-tcc-identity"]
        .into_iter()
        .find(|unsupported| arguments.iter().any(|argument| argument == unsupported))
}

fn validate_desktop_compatibility(
    executable: &str,
    detected_version: &str,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), HermesDesktopError> {
    validate_desktop_version(detected_version, allow_unsupported, allow_untested)?;
    let output = Command::new(executable)
        .args(["desktop", "--help"])
        .output()
        .map_err(HermesDesktopError::CapabilityProbe)?;
    if !output.status.success() {
        return Err(HermesDesktopError::CapabilityProbeFailed(
            output.status.code(),
        ));
    }
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let missing = missing_desktop_capabilities(&help);
    if !missing.is_empty() {
        return Err(HermesDesktopError::MissingDesktopCapabilities(
            missing.join(", "),
        ));
    }
    Ok(())
}

fn validate_desktop_version(
    detected_version: &str,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), HermesDesktopError> {
    let entry = desktop_compatibility(DesktopHarnessKind::Hermes)?;
    let version = extract_semver(detected_version);
    match classify_desktop_version(&entry, version.as_ref()) {
        DesktopCompatibilityStatus::ContractOnly => eprintln!(
            "warning: Hermes Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
        ),
        DesktopCompatibilityStatus::OlderUnsupported if !allow_unsupported => {
            let (Some(detected), Some(minimum)) =
                (version.as_ref(), entry.minimum_app_version.as_ref())
            else {
                return Err(HermesDesktopError::InvalidCompatibilityEvidence);
            };
            return Err(HermesDesktopError::DesktopVersionUnsupported {
                detected: detected.clone(),
                minimum: minimum.clone(),
            });
        }
        DesktopCompatibilityStatus::OlderUnsupported => {
            eprintln!("warning: running an older unsupported Hermes Desktop version");
        }
        DesktopCompatibilityStatus::NewerUntested if !allow_untested => {
            let (Some(detected), Some(last)) =
                (version.as_ref(), entry.last_compatible_app_version.as_ref())
            else {
                return Err(HermesDesktopError::InvalidCompatibilityEvidence);
            };
            return Err(HermesDesktopError::DesktopVersionUntested {
                detected: detected.clone(),
                last: last.clone(),
            });
        }
        DesktopCompatibilityStatus::NewerUntested => {
            eprintln!(
                "warning: this Hermes Desktop version is newer than the local compatibility evidence"
            );
        }
        DesktopCompatibilityStatus::Unavailable => {
            return Err(HermesDesktopError::DesktopUnavailable);
        }
        DesktopCompatibilityStatus::Tested => {}
    }
    debug_assert_ne!(entry.evidence, DesktopCompatibilityEvidence::Unavailable);
    Ok(())
}

fn missing_desktop_capabilities(help: &str) -> Vec<&'static str> {
    ["--source", "--skip-build", "--cwd"]
        .into_iter()
        .filter(|flag| !help.contains(flag))
        .collect()
}

fn extract_semver(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|candidate| {
        let candidate = candidate.trim_matches(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-' && character != '+'
        });
        Version::parse(candidate).ok()
    })
}

fn select_model<'a>(
    models: &'a [CodingModelProfile],
    requested: Option<&str>,
) -> Result<&'a str, HermesDesktopError> {
    let selected = requested.unwrap_or(DEFAULT_MODEL_ID);
    if let Some(model) = models.iter().find(|model| model.id == selected) {
        return Ok(&model.id);
    }
    if requested.is_some() {
        return Err(HermesDesktopError::ModelUnavailable {
            model: selected.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        });
    }
    models
        .first()
        .map(|model| model.id.as_str())
        .ok_or(HermesDesktopError::EmptyModelCatalog)
}

fn desktop_arguments(paths: &DesktopPaths, user_arguments: &[String]) -> Vec<String> {
    let mut arguments = vec!["desktop".to_owned()];
    if packaged_desktop_exists(paths)
        && !has_build_selection(user_arguments)
        && !has_alternate_hermes_root(user_arguments)
    {
        arguments.push("--skip-build".to_owned());
    }
    arguments.extend(user_arguments.iter().cloned());
    arguments
}

fn has_alternate_hermes_root(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--hermes-root" || argument.starts_with("--hermes-root="))
}

fn has_build_selection(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--source" | "--skip-build" | "--force-build" | "--build-only"
        )
    })
}

fn packaged_desktop_exists(paths: &DesktopPaths) -> bool {
    packaged_desktop_candidates(&paths.install_root)
        .iter()
        .any(|candidate| candidate.is_file())
}

async fn bind_stable_gateway(
    paths: &DesktopPaths,
    ownership: &mut OwnershipReceipt,
) -> Result<TcpListener, HermesDesktopError> {
    let listener = match ownership.gateway_port {
        Some(port) => TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|source| HermesDesktopError::StablePortUnavailable { port, source })?,
        None => TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HermesDesktopError::BindGateway)?,
    };
    if ownership.gateway_port.is_none() {
        ownership.gateway_port = Some(
            listener
                .local_addr()
                .map_err(HermesDesktopError::BindGateway)?
                .port(),
        );
        write_json_private(&paths.ownership_receipt, ownership)?;
    }
    Ok(listener)
}

fn spawn_desktop(
    executable: &str,
    arguments: &[String],
    paths: &DesktopPaths,
    working_directory: &Path,
) -> Result<Child, HermesDesktopError> {
    let mut command = TokioCommand::new(executable);
    command
        .args(arguments)
        .current_dir(working_directory)
        .env("HERMES_HOME", &paths.hermes_home)
        .env_remove("NAN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("CUSTOM_BASE_URL")
        .env_remove("HERMES_INFERENCE_MODEL")
        .env_remove("HERMES_INFERENCE_PROVIDER")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command.spawn().map_err(HermesDesktopError::Launch)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleCompletion {
    Closed(i32),
    PreserveRecovery(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateWaitCompletion {
    Finished { interrupt_seen: bool },
    PreserveRecovery(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RelaunchWaitCompletion {
    Running(DesktopProcess),
    PreserveRecovery(i32),
    TimedOut,
}

async fn supervise_desktop(
    child: &mut Child,
    mut gateway: Option<&mut RunningChatCompletionsGateway>,
    paths: &DesktopPaths,
    marker_before_launch: Option<MarkerFingerprint>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<LifecycleCompletion, HermesDesktopError> {
    let initial_status = tokio::select! {
        status = child.wait() => status.map_err(HermesDesktopError::Wait)?,
        signal = signals.recv() => {
            let exit_code = signal.unwrap_or(143);
            terminate_desktop_or_child(child).await?;
            return Ok(LifecycleCompletion::Closed(exit_code));
        }
        gateway_result = wait_for_gateway(&mut gateway) => {
            let error = gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited);
            terminate_desktop_or_child(child).await?;
            return Err(error);
        }
    };

    if !update_started(paths, marker_before_launch) {
        if let Some(process) = running_desktop()? {
            eprintln!("Hermes Desktop's launcher exited; continuing to supervise the running app.");
            return supervise_running_desktop(process, &mut gateway, signals).await;
        }
        return Ok(LifecycleCompletion::Closed(exit_code(initial_status)));
    }

    eprintln!(
        "Hermes Desktop is updating. NaN will keep the local gateway and managed profile active."
    );
    let interrupt_seen = match wait_for_update(paths, &mut gateway, signals).await? {
        UpdateWaitCompletion::Finished { interrupt_seen } => interrupt_seen,
        UpdateWaitCompletion::PreserveRecovery(exit_code) => {
            return Ok(LifecycleCompletion::PreserveRecovery(exit_code));
        }
    };
    let process = match wait_for_relaunch(&mut gateway, signals, interrupt_seen).await? {
        RelaunchWaitCompletion::Running(process) => process,
        RelaunchWaitCompletion::PreserveRecovery(exit_code) => {
            return Ok(LifecycleCompletion::PreserveRecovery(exit_code));
        }
        RelaunchWaitCompletion::TimedOut => return Err(HermesDesktopError::DidNotRelaunch),
    };
    eprintln!("Hermes Desktop update completed; continuing the same NaN session.");
    supervise_running_desktop(process, &mut gateway, signals).await
}

async fn supervise_running_desktop(
    mut process: DesktopProcess,
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<LifecycleCompletion, HermesDesktopError> {
    loop {
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {
                if !process_is_same(&process)? {
                    if let Some(replacement) = running_desktop()? {
                        process = replacement;
                    } else {
                        return Ok(LifecycleCompletion::Closed(0));
                    }
                }
            }
            signal = signals.recv() => {
                let exit_code = signal.unwrap_or(143);
                terminate_desktop().await?;
                return Ok(LifecycleCompletion::Closed(exit_code));
            }
            gateway_result = wait_for_gateway(gateway) => {
                let error = gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited);
                terminate_desktop().await?;
                return Err(error);
            }
        }
    }
}

async fn wait_for_gateway(
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
) -> Result<(), HermesDesktopError> {
    match gateway.as_deref_mut() {
        Some(gateway) => gateway.wait().await.map_err(HermesDesktopError::Gateway),
        None => std::future::pending().await,
    }
}

async fn wait_for_update(
    paths: &DesktopPaths,
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<UpdateWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    let mut interrupt_seen = false;
    let mut stale_since = None;
    loop {
        if !paths.update_marker.exists() {
            return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
        }
        if live_update_owner(&paths.update_marker)?.is_some() {
            stale_since = None;
        } else {
            let since = stale_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(5) {
                return Ok(UpdateWaitCompletion::Finished { interrupt_seen });
            }
        }
        if started.elapsed() >= UPDATE_WAIT_TIMEOUT {
            return Err(HermesDesktopError::UpdateTimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(UPDATE_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting while the Hermes Desktop updater continues. Run `nan hermes-desktop --restore` after the update finishes.");
                    return Ok(UpdateWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes Desktop is still updating. Press Ctrl+C again to exit NaN while the updater continues.");
            }
            gateway_result = wait_for_gateway(gateway) => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}

async fn wait_for_relaunch(
    gateway: &mut Option<&mut RunningChatCompletionsGateway>,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    mut interrupt_seen: bool,
) -> Result<RelaunchWaitCompletion, HermesDesktopError> {
    let started = Instant::now();
    loop {
        if let Some(process) = running_desktop()? {
            return Ok(RelaunchWaitCompletion::Running(process));
        }
        if started.elapsed() >= RELAUNCH_WAIT_TIMEOUT {
            return Ok(RelaunchWaitCompletion::TimedOut);
        }
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                if update_interrupt_requests_exit(code, &mut interrupt_seen) {
                    eprintln!("NaN is exiting before Hermes Desktop relaunches. Run `nan hermes-desktop --restore` after the update finishes.");
                    return Ok(RelaunchWaitCompletion::PreserveRecovery(code));
                }
                eprintln!("Hermes has finished updating and is relaunching. Press Ctrl+C again to exit NaN and preserve recovery state.");
            }
            gateway_result = wait_for_gateway(gateway) => {
                return Err(gateway_result.err().unwrap_or(HermesDesktopError::GatewayExited));
            }
        }
    }
}

fn update_interrupt_requests_exit(code: i32, interrupt_seen: &mut bool) -> bool {
    if code != 130 || *interrupt_seen {
        true
    } else {
        *interrupt_seen = true;
        false
    }
}

fn update_started(paths: &DesktopPaths, marker_before_launch: Option<MarkerFingerprint>) -> bool {
    if live_update_owner(&paths.update_marker)
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }
    let after = marker_fingerprint(&paths.update_marker);
    after.is_some() && after != marker_before_launch
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn termination_signals() -> tokio::sync::mpsc::UnboundedReceiver<i32> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut interrupt) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            else {
                return;
            };
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                return;
            };
            loop {
                tokio::select! {
                    value = interrupt.recv() => {
                        if value.is_none() || sender.send(130).is_err() { return; }
                    }
                    value = terminate.recv() => {
                        if value.is_none() || sender.send(143).is_err() { return; }
                    }
                }
            }
        }
        #[cfg(not(unix))]
        loop {
            if tokio::signal::ctrl_c().await.is_err() || sender.send(130).is_err() {
                return;
            }
        }
    });
    receiver
}

async fn terminate_desktop_or_child(child: &mut Child) -> Result<(), HermesDesktopError> {
    if running_desktop()?.is_some() {
        terminate_desktop().await
    } else {
        child.start_kill().map_err(HermesDesktopError::Terminate)?;
        terminate_desktop().await
    }
}

async fn terminate_desktop() -> Result<(), HermesDesktopError> {
    let mut quiet_since = None;
    loop {
        if let Some(process) = running_desktop()? {
            let _ = desktop_quiescence_reached(
                &mut quiet_since,
                Instant::now(),
                true,
                DESKTOP_QUIESCENCE_INTERVAL,
            );
            terminate_desktop_process(&process).await?;
        } else {
            if desktop_quiescence_reached(
                &mut quiet_since,
                Instant::now(),
                false,
                DESKTOP_QUIESCENCE_INTERVAL,
            ) {
                return Ok(());
            }
            tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
        }
    }
}

fn desktop_quiescence_reached(
    quiet_since: &mut Option<Instant>,
    now: Instant,
    process_running: bool,
    interval: Duration,
) -> bool {
    if process_running {
        *quiet_since = None;
        return false;
    }
    let since = quiet_since.get_or_insert(now);
    now.duration_since(*since) >= interval
}

async fn terminate_desktop_process(process: &DesktopProcess) -> Result<(), HermesDesktopError> {
    request_process_termination(process)?;
    for _ in 0..60 {
        if !process_is_same(process)? {
            return Ok(());
        }
        tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
    }
    force_process_termination(process)?;
    for _ in 0..40 {
        if !process_is_same(process)? {
            return Ok(());
        }
        tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
    }
    Err(HermesDesktopError::DidNotTerminate)
}

#[cfg(target_os = "macos")]
fn request_process_termination(process: &DesktopProcess) -> Result<(), HermesDesktopError> {
    let status = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application id \"com.nousresearch.hermes\" to quit",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(HermesDesktopError::Terminate)?;
    if status.success() {
        Ok(())
    } else {
        terminate_pid(process.pid, false)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn request_process_termination(process: &DesktopProcess) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, false)
}

#[cfg(windows)]
fn request_process_termination(process: &DesktopProcess) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, false)
}

fn force_process_termination(process: &DesktopProcess) -> Result<(), HermesDesktopError> {
    terminate_pid(process.pid, true)
}

#[cfg(unix)]
fn terminate_pid(pid: u32, force: bool) -> Result<(), HermesDesktopError> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .status()
        .map_err(HermesDesktopError::Terminate)?;
    if status.success() || !pid_is_alive(pid)? {
        Ok(())
    } else {
        Err(HermesDesktopError::TerminateFailed(status.code()))
    }
}

#[cfg(windows)]
fn terminate_pid(pid: u32, force: bool) -> Result<(), HermesDesktopError> {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let status = command.status().map_err(HermesDesktopError::Terminate)?;
    if status.success() || !pid_is_alive(pid)? {
        Ok(())
    } else {
        Err(HermesDesktopError::TerminateFailed(status.code()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopProcess {
    pid: u32,
    started: String,
}

#[cfg(unix)]
fn running_desktop() -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let output = Command::new("/bin/ps")
        .args(["-ww", "-axo", "pid=,lstart=,command="])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(HermesDesktopError::ProcessCheckFailed(output.status.code()));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        let trimmed = line.trim_start();
        let Some((pid, rest)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let rest = rest.trim_start();
        if rest.len() < 24 {
            continue;
        }
        let started = rest[..24].trim().to_owned();
        let command = rest[24..].trim();
        if desktop_main_command(command) {
            return Ok(Some(DesktopProcess { pid, started }));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn desktop_main_command(command: &str) -> bool {
    !command.contains("--type=")
        && (command.contains("/Hermes.app/Contents/MacOS/Hermes")
            || command.contains("/apps/desktop/release/linux-")
                && (command.ends_with("/hermes") || command.ends_with("/Hermes"))
            || command.contains("/apps/desktop/node_modules/electron/")
                && command.contains("apps/desktop"))
}

#[cfg(windows)]
fn running_desktop() -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let script = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'Hermes.exe' -or ($_.Name -eq 'electron.exe' -and $_.CommandLine -match '[\\/]apps[\\/]desktop') } | Select-Object ProcessId,CreationDate,Name,CommandLine | ConvertTo-Json -Compress";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(HermesDesktopError::ProcessCheckFailed(output.status.code()));
    }
    let value = String::from_utf8_lossy(&output.stdout);
    if value.trim().is_empty() {
        return Ok(None);
    }
    parse_windows_process_listing(&value)
}

#[cfg(any(windows, test))]
fn parse_windows_process_listing(
    value: &str,
) -> Result<Option<DesktopProcess>, HermesDesktopError> {
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(HermesDesktopError::ParseProcessListing)?;
    let records = match &parsed {
        serde_json::Value::Array(records) => records.iter().collect::<Vec<_>>(),
        serde_json::Value::Object(_) => vec![&parsed],
        _ => return Err(HermesDesktopError::InvalidProcessListing),
    };
    let mut main_processes = Vec::new();
    for record in records {
        let name = record["Name"]
            .as_str()
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        let command = record["CommandLine"]
            .as_str()
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        if !windows_desktop_main_process(name, command) {
            continue;
        }
        let pid = record["ProcessId"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(HermesDesktopError::InvalidProcessListing)?;
        let started = record["CreationDate"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        main_processes.push(DesktopProcess { pid, started });
    }
    match main_processes.len() {
        0 => Ok(None),
        1 => Ok(main_processes.pop()),
        _ => Err(HermesDesktopError::AmbiguousDesktopProcesses),
    }
}

#[cfg(any(windows, test))]
fn windows_desktop_main_process(name: &str, command: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let command = command.to_ascii_lowercase();
    if command.contains("--type=") {
        return false;
    }
    name == "hermes.exe"
        || name == "electron.exe"
            && (command.contains("/apps/desktop") || command.contains("\\apps\\desktop"))
}

fn process_is_same(process: &DesktopProcess) -> Result<bool, HermesDesktopError> {
    Ok(running_desktop()?.as_ref() == Some(process))
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> Result<bool, HermesDesktopError> {
    let status = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(HermesDesktopError::ProcessCheck)?;
    Ok(status.success())
}

#[cfg(windows)]
fn pid_is_alive(pid: u32) -> Result<bool, HermesDesktopError> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map_err(HermesDesktopError::ProcessCheck)?;
    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")))
}

fn live_update_owner(path: &Path) -> Result<Option<u32>, HermesDesktopError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(HermesDesktopError::ReadUpdateMarker(error)),
    };
    let Some(pid) = contents
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok())
    else {
        return Ok(None);
    };
    pid_is_alive(pid).map(|alive| alive.then_some(pid))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerFingerprint {
    modified: Option<SystemTime>,
    length: u64,
}

fn marker_fingerprint(path: &Path) -> Option<MarkerFingerprint> {
    let metadata = fs::metadata(path).ok()?;
    Some(MarkerFingerprint {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}

#[derive(Debug)]
struct DesktopPaths {
    state_directory: PathBuf,
    lock: PathBuf,
    ownership_receipt: PathBuf,
    session_receipt: PathBuf,
    backup_directory: PathBuf,
    hermes_home: PathBuf,
    install_root: PathBuf,
    profiles_root: PathBuf,
    parked_profiles_root: PathBuf,
    recovered_profiles_root: PathBuf,
    managed_profile: PathBuf,
    parked_profile: PathBuf,
    active_profile: PathBuf,
    update_marker: PathBuf,
}

impl DesktopPaths {
    fn from_environment() -> Result<Self, HermesDesktopError> {
        let state_directory = config_directory()
            .ok_or(HermesDesktopError::MissingStateDirectory)?
            .join("hermes-desktop");
        if !state_directory.is_absolute() {
            return Err(HermesDesktopError::InvalidStateDirectory);
        }
        let user_home = user_home().ok_or(HermesDesktopError::MissingHomeDirectory)?;
        let hermes_home = resolve_hermes_home(&user_home);
        if !hermes_home.is_absolute() {
            return Err(HermesDesktopError::InvalidHermesHome);
        }
        let user_data = desktop_user_data(&user_home);
        let profiles_root = hermes_home.join("profiles");
        let parked_profiles_root = profiles_root.join(PARKED_PROFILES_DIRECTORY);
        Ok(Self {
            lock: state_directory.join("session.lock"),
            ownership_receipt: state_directory.join("ownership.json"),
            session_receipt: state_directory.join("session.json"),
            backup_directory: state_directory.join("session-backups"),
            install_root: hermes_home.join("hermes-agent"),
            managed_profile: profiles_root.join(PROFILE_NAME),
            parked_profile: parked_profiles_root.join(PROFILE_NAME),
            active_profile: user_data.join("active-profile.json"),
            update_marker: hermes_home.join(".hermes-update-in-progress"),
            recovered_profiles_root: parked_profiles_root.join(RECOVERED_PROFILES_DIRECTORY),
            parked_profiles_root,
            profiles_root,
            hermes_home,
            state_directory,
        })
    }

    #[cfg(test)]
    fn for_test(root: &Path) -> Self {
        let state_directory = root.join("state");
        let hermes_home = root.join(".hermes");
        let profiles_root = hermes_home.join("profiles");
        let parked_profiles_root = profiles_root.join(PARKED_PROFILES_DIRECTORY);
        Self {
            lock: state_directory.join("session.lock"),
            ownership_receipt: state_directory.join("ownership.json"),
            session_receipt: state_directory.join("session.json"),
            backup_directory: state_directory.join("session-backups"),
            install_root: hermes_home.join("hermes-agent"),
            managed_profile: profiles_root.join(PROFILE_NAME),
            parked_profile: parked_profiles_root.join(PROFILE_NAME),
            active_profile: root.join("user-data/active-profile.json"),
            update_marker: hermes_home.join(".hermes-update-in-progress"),
            recovered_profiles_root: parked_profiles_root.join(RECOVERED_PROFILES_DIRECTORY),
            parked_profiles_root,
            profiles_root,
            hermes_home,
            state_directory,
        }
    }
}

fn resolve_hermes_home(user_home: &Path) -> PathBuf {
    if let Some(explicit) = std::env::var_os("HERMES_HOME") {
        return PathBuf::from(explicit);
    }
    #[cfg(windows)]
    {
        return choose_windows_hermes_home(
            user_home,
            windows_user_scoped_hermes_home(),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        );
    }
    #[cfg(not(windows))]
    {
        user_home.join(".hermes")
    }
}

#[cfg(any(windows, test))]
fn choose_windows_hermes_home(
    user_home: &Path,
    user_scoped: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
) -> PathBuf {
    if let Some(user_scoped) = user_scoped.filter(|path| !path.as_os_str().is_empty()) {
        return user_scoped;
    }
    let modern = local_app_data
        .unwrap_or_else(|| user_home.join("AppData/Local"))
        .join("hermes");
    let legacy = user_home.join(".hermes");
    if !modern.is_dir() && legacy.is_dir() {
        legacy
    } else {
        modern
    }
}

#[cfg(windows)]
fn windows_user_scoped_hermes_home() -> Option<PathBuf> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('HERMES_HOME','User')",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn desktop_user_data(user_home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        user_home.join("Library/Application Support/Hermes")
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Hermes"))
            .unwrap_or_else(|| user_home.join("AppData/Roaming/Hermes"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home.join(".config"))
            .join("Hermes")
    }
}

fn packaged_desktop_candidates(install_root: &Path) -> Vec<PathBuf> {
    let release = install_root.join("apps/desktop/release");
    #[cfg(target_os = "macos")]
    {
        let mut candidates = Vec::new();
        if let Ok(entries) = fs::read_dir(&release) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("mac") {
                    candidates.push(entry.path().join("Hermes.app/Contents/MacOS/Hermes"));
                }
            }
        }
        candidates
    }
    #[cfg(windows)]
    {
        ["win-unpacked", "win-ia32-unpacked", "win-arm64-unpacked"]
            .into_iter()
            .map(|directory| release.join(directory).join("Hermes.exe"))
            .collect()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        ["linux-unpacked", "linux-arm64-unpacked"]
            .into_iter()
            .flat_map(|directory| {
                let directory = release.join(directory);
                ["hermes", "Hermes"]
                    .into_iter()
                    .map(move |binary| directory.join(binary))
            })
            .collect()
    }
}

struct SessionLock {
    _file: File,
}

impl SessionLock {
    fn acquire(paths: &DesktopPaths) -> Result<Self, HermesDesktopError> {
        fs::create_dir_all(&paths.state_directory)
            .map_err(HermesDesktopError::CreateStateDirectory)?;
        restrict_path(&paths.state_directory, PrivatePathKind::Directory)
            .map_err(HermesDesktopError::ProtectStateDirectory)?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&paths.lock)
            .map_err(HermesDesktopError::OpenLock)?;
        nan_harness_private_fs::restrict_file(&mut file)
            .map_err(HermesDesktopError::ProtectLock)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(HermesDesktopError::ConcurrentSession),
            Err(fs::TryLockError::Error(error)) => Err(HermesDesktopError::Lock(error)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerMarker {
    schema_version: u8,
    owner_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnershipReceipt {
    schema_version: u8,
    owner_id: String,
    profile_name: String,
    gateway_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedProfileLocation {
    Active,
    Parked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfilePathKind {
    Missing,
    Directory,
    RegularFile,
    Other,
}

fn ensure_managed_profile(paths: &DesktopPaths) -> Result<OwnershipReceipt, HermesDesktopError> {
    if let Some((ownership, location)) = locate_managed_profile(paths)? {
        if location == ManagedProfileLocation::Parked {
            ensure_profile_guard(paths, &ownership)?;
        }
        return Ok(ownership);
    }
    create_managed_profile(paths)
}

fn locate_managed_profile(
    paths: &DesktopPaths,
) -> Result<Option<(OwnershipReceipt, ManagedProfileLocation)>, HermesDesktopError> {
    let active = profile_path_kind(&paths.managed_profile)?;
    let parked = profile_path_kind(&paths.parked_profile)?;
    if parked != ProfilePathKind::Missing && parked != ProfilePathKind::Directory {
        return Err(HermesDesktopError::ParkedProfileOwnershipMismatch);
    }
    if active == ProfilePathKind::Directory && parked == ProfilePathKind::Directory {
        return Err(HermesDesktopError::ManagedProfileConflict);
    }
    if active == ProfilePathKind::Other {
        return Err(HermesDesktopError::UnmanagedNanProfile);
    }
    let ownership = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)?;
    let selected = match (active, parked) {
        (ProfilePathKind::Directory, ProfilePathKind::Missing) => {
            Some((&paths.managed_profile, ManagedProfileLocation::Active))
        }
        (ProfilePathKind::Missing | ProfilePathKind::RegularFile, ProfilePathKind::Directory) => {
            Some((&paths.parked_profile, ManagedProfileLocation::Parked))
        }
        _ => None,
    };
    let Some((profile, location)) = selected else {
        return if ownership.is_some() {
            Err(HermesDesktopError::ManagedProfileMissing)
        } else if active == ProfilePathKind::RegularFile {
            Err(HermesDesktopError::UnmanagedNanProfile)
        } else {
            Ok(None)
        };
    };
    let marker = read_optional_json::<OwnerMarker>(&profile.join(OWNER_MARKER_FILE))?;
    let Some(marker) = marker else {
        return Err(match location {
            ManagedProfileLocation::Active => HermesDesktopError::UnmanagedNanProfile,
            ManagedProfileLocation::Parked => HermesDesktopError::ParkedProfileOwnershipMismatch,
        });
    };
    let ownership = match ownership {
        Some(ownership) => {
            validate_ownership(&ownership, &marker)?;
            ownership
        }
        None => recover_ownership(paths, marker, location)?,
    };
    if active == ProfilePathKind::RegularFile {
        let guard =
            read_profile_guard(paths)?.ok_or(HermesDesktopError::ProfileGuardOwnershipMismatch)?;
        if guard.schema_version != OWNERSHIP_SCHEMA_VERSION || guard.owner_id != ownership.owner_id
        {
            return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
        }
    }
    Ok(Some((ownership, location)))
}

fn profile_path_kind(path: &Path) -> Result<ProfilePathKind, HermesDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(ProfilePathKind::Directory),
        Ok(metadata) if metadata.file_type().is_file() => Ok(ProfilePathKind::RegularFile),
        Ok(_) => Ok(ProfilePathKind::Other),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ProfilePathKind::Missing),
        Err(error) => Err(HermesDesktopError::ReadFile(error)),
    }
}

fn read_profile_guard(paths: &DesktopPaths) -> Result<Option<OwnerMarker>, HermesDesktopError> {
    let Some(contents) = read_optional(&paths.managed_profile)? else {
        return Ok(None);
    };
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(|_| HermesDesktopError::ProfileGuardOwnershipMismatch)
}

fn activate_managed_profile(
    paths: &DesktopPaths,
    expected: &OwnershipReceipt,
) -> Result<(), HermesDesktopError> {
    let Some((ownership, location)) = locate_managed_profile(paths)? else {
        return Err(HermesDesktopError::ManagedProfileMissing);
    };
    if &ownership != expected {
        return Err(HermesDesktopError::OwnershipMismatch);
    }
    if location == ManagedProfileLocation::Active {
        return Ok(());
    }
    fs::create_dir_all(&paths.profiles_root).map_err(HermesDesktopError::CreateProfile)?;
    remove_profile_guard(paths)?;
    if let Err(error) = fs::rename(&paths.parked_profile, &paths.managed_profile) {
        let _ = ensure_profile_guard(paths, &ownership);
        return Err(HermesDesktopError::ActivateProfile(error));
    }
    Ok(())
}

fn park_managed_profile_if_owned(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    match locate_managed_profile(paths)? {
        Some((_, ManagedProfileLocation::Active)) => park_managed_profile(paths),
        Some((ownership, ManagedProfileLocation::Parked)) => {
            ensure_profile_guard(paths, &ownership)
        }
        None => Ok(()),
    }
}

fn park_managed_profile(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some((ownership, location)) = locate_managed_profile(paths)? else {
        return Err(HermesDesktopError::ManagedProfileMissing);
    };
    if location == ManagedProfileLocation::Parked {
        return ensure_profile_guard(paths, &ownership);
    }
    reset_managed_active_profile(paths)?;
    fs::create_dir_all(&paths.parked_profiles_root)
        .map_err(HermesDesktopError::CreateParkingDirectory)?;
    restrict_path(&paths.parked_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectParkingDirectory)?;
    fs::rename(&paths.managed_profile, &paths.parked_profile)
        .map_err(HermesDesktopError::ParkProfile)?;
    if let Err(error) = ensure_profile_guard(paths, &ownership) {
        let _ = fs::rename(&paths.parked_profile, &paths.managed_profile);
        return Err(error);
    }
    Ok(())
}

fn ensure_profile_guard(
    paths: &DesktopPaths,
    ownership: &OwnershipReceipt,
) -> Result<(), HermesDesktopError> {
    match profile_path_kind(&paths.managed_profile)? {
        ProfilePathKind::RegularFile => {
            let guard = read_profile_guard(paths)?
                .ok_or(HermesDesktopError::ProfileGuardOwnershipMismatch)?;
            if guard.schema_version == OWNERSHIP_SCHEMA_VERSION
                && guard.owner_id == ownership.owner_id
            {
                return Ok(());
            }
            return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
        }
        ProfilePathKind::Missing => {}
        ProfilePathKind::Directory => return Err(HermesDesktopError::ManagedProfileConflict),
        ProfilePathKind::Other => {
            return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
        }
    }
    let marker = OwnerMarker {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: ownership.owner_id.clone(),
    };
    let payload = serde_json::to_vec_pretty(&marker).map_err(HermesDesktopError::Serialize)?;
    let mut file =
        open_private_new(&paths.managed_profile).map_err(HermesDesktopError::CreateProfileGuard)?;
    if let Err(error) = std::io::Write::write_all(&mut file, &payload) {
        drop(file);
        let _ = fs::remove_file(&paths.managed_profile);
        return Err(HermesDesktopError::WriteProfileGuard(error));
    }
    Ok(())
}

fn remove_profile_guard(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    match profile_path_kind(&paths.managed_profile)? {
        ProfilePathKind::Missing => Ok(()),
        ProfilePathKind::RegularFile => {
            fs::remove_file(&paths.managed_profile).map_err(HermesDesktopError::RemoveProfileGuard)
        }
        ProfilePathKind::Directory => Err(HermesDesktopError::ManagedProfileConflict),
        ProfilePathKind::Other => Err(HermesDesktopError::ProfileGuardOwnershipMismatch),
    }
}

fn quarantine_recreated_profile_for_restore(
    paths: &DesktopPaths,
) -> Result<(), HermesDesktopError> {
    if profile_path_kind(&paths.managed_profile)? != ProfilePathKind::Directory
        || profile_path_kind(&paths.parked_profile)? != ProfilePathKind::Directory
        || paths.managed_profile.join("config.yaml").exists()
        || read_optional_json::<OwnerMarker>(&paths.managed_profile.join(OWNER_MARKER_FILE))?
            .is_some()
    {
        return Ok(());
    }
    let Some(ownership) = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)? else {
        return Ok(());
    };
    let Some(marker) =
        read_optional_json::<OwnerMarker>(&paths.parked_profile.join(OWNER_MARKER_FILE))?
    else {
        return Ok(());
    };
    validate_ownership(&ownership, &marker)?;
    fs::create_dir_all(&paths.recovered_profiles_root)
        .map_err(HermesDesktopError::CreateRecoveryDirectory)?;
    restrict_path(&paths.recovered_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectRecoveryDirectory)?;
    let recovered = paths
        .recovered_profiles_root
        .join(format!("{PROFILE_NAME}-{}", random_id()?));
    fs::rename(&paths.managed_profile, &recovered)
        .map_err(HermesDesktopError::QuarantineRecreatedProfile)?;
    if let Err(error) = ensure_profile_guard(paths, &ownership) {
        let _ = fs::rename(&recovered, &paths.managed_profile);
        return Err(error);
    }
    eprintln!(
        "warning: Hermes Desktop recreated an empty 'nan' profile from cached UI state; NaN preserved it in the private recovery area and restored the visibility guard."
    );
    Ok(())
}

fn reset_managed_active_profile(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some(contents) = read_optional(&paths.active_profile)? else {
        return Ok(());
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&contents) else {
        return Ok(());
    };
    if value.get("profile").and_then(serde_json::Value::as_str) == Some(PROFILE_NAME) {
        let default = serde_json::to_vec_pretty(&json!({"profile": "default"}))
            .map_err(HermesDesktopError::Serialize)?;
        write_private(&paths.active_profile, &default)?;
    }
    Ok(())
}

fn recover_ownership(
    paths: &DesktopPaths,
    marker: OwnerMarker,
    location: ManagedProfileLocation,
) -> Result<OwnershipReceipt, HermesDesktopError> {
    if marker.schema_version != OWNERSHIP_SCHEMA_VERSION || marker.owner_id == "diagnostic" {
        return Err(match location {
            ManagedProfileLocation::Active => HermesDesktopError::UnmanagedNanProfile,
            ManagedProfileLocation::Parked => HermesDesktopError::ParkedProfileOwnershipMismatch,
        });
    }
    let ownership = OwnershipReceipt {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: marker.owner_id,
        profile_name: PROFILE_NAME.to_owned(),
        gateway_port: None,
    };
    write_json_private(&paths.ownership_receipt, &ownership)?;
    Ok(ownership)
}

fn create_managed_profile(paths: &DesktopPaths) -> Result<OwnershipReceipt, HermesDesktopError> {
    fs::create_dir_all(&paths.parked_profiles_root)
        .map_err(HermesDesktopError::CreateParkingDirectory)?;
    restrict_path(&paths.parked_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectParkingDirectory)?;
    fs::create_dir(&paths.parked_profile).map_err(HermesDesktopError::CreateProfile)?;
    restrict_path(&paths.parked_profile, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectProfile)?;
    let owner_id = random_id()?;
    let marker = OwnerMarker {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: owner_id.clone(),
    };
    let ownership = OwnershipReceipt {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id,
        profile_name: PROFILE_NAME.to_owned(),
        gateway_port: None,
    };
    let result = (|| {
        write_json_private(&paths.parked_profile.join(OWNER_MARKER_FILE), &marker)?;
        write_json_private(&paths.ownership_receipt, &ownership)?;
        ensure_profile_guard(paths, &ownership)?;
        Ok::<(), HermesDesktopError>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&paths.parked_profile);
        if read_profile_guard(paths)
            .ok()
            .flatten()
            .is_some_and(|guard| guard.owner_id == ownership.owner_id)
        {
            let _ = fs::remove_file(&paths.managed_profile);
        }
        if read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)
            .ok()
            .flatten()
            .is_some_and(|receipt| receipt.owner_id == ownership.owner_id)
        {
            let _ = fs::remove_file(&paths.ownership_receipt);
        }
        return Err(error);
    }
    Ok(ownership)
}

fn validate_ownership(
    ownership: &OwnershipReceipt,
    marker: &OwnerMarker,
) -> Result<(), HermesDesktopError> {
    if ownership.schema_version != OWNERSHIP_SCHEMA_VERSION
        || marker.schema_version != OWNERSHIP_SCHEMA_VERSION
    {
        return Err(HermesDesktopError::UnsupportedOwnershipSchema);
    }
    if ownership.profile_name != PROFILE_NAME || ownership.owner_id != marker.owner_id {
        return Err(HermesDesktopError::OwnershipMismatch);
    }
    Ok(())
}

fn remove_legacy_profile_display_name(profile: &Path) -> Result<(), HermesDesktopError> {
    let path = profile.join("profile.yaml");
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(HermesDesktopError::ReadFile(error)),
    };
    if matches!(
        contents.as_slice(),
        b"display_name: NaN" | b"display_name: NaN\n"
    ) {
        remove_if_exists(&path).map_err(HermesDesktopError::RemoveProfileMetadata)?;
    }
    Ok(())
}

fn create_diagnostic_profile(paths: &DesktopPaths) -> Result<PathBuf, HermesDesktopError> {
    fs::create_dir_all(&paths.profiles_root).map_err(HermesDesktopError::CreateProfile)?;
    let name = format!("{DIAGNOSTIC_PROFILE_PREFIX}{}", random_id()?.to_lowercase());
    let profile = paths.profiles_root.join(name);
    fs::create_dir(&profile).map_err(HermesDesktopError::CreateProfile)?;
    restrict_path(&profile, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectProfile)?;
    write_json_private(
        &profile.join(OWNER_MARKER_FILE),
        &OwnerMarker {
            schema_version: OWNERSHIP_SCHEMA_VERSION,
            owner_id: "diagnostic".to_owned(),
        },
    )?;
    Ok(profile)
}

fn cleanup_stale_diagnostic_profiles(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let entries = match fs::read_dir(&paths.profiles_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(HermesDesktopError::ReadProfiles(error)),
    };
    for entry in entries {
        let entry = entry.map_err(HermesDesktopError::ReadProfiles)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(DIAGNOSTIC_PROFILE_PREFIX)
        {
            continue;
        }
        let marker = read_optional_json::<OwnerMarker>(&entry.path().join(OWNER_MARKER_FILE))?;
        if marker.as_ref().is_some_and(|marker| {
            marker.schema_version == OWNERSHIP_SCHEMA_VERSION && marker.owner_id == "diagnostic"
        }) {
            fs::remove_dir_all(entry.path()).map_err(HermesDesktopError::RemoveProfile)?;
        }
    }
    Ok(())
}

fn write_profile_config(
    profile: &Path,
    base_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
    web_search_enabled: bool,
) -> Result<(), HermesDesktopError> {
    let path = profile.join("config.yaml");
    reject_profile_symlink(&path)?;
    let existing = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(HermesDesktopError::ReadProfileConfig(error)),
    };
    let model_block = format!(
        "model:\n  default: {}\n  provider: nan",
        yaml_string(selected_model)
    );
    let provider_block = render_hermes_desktop_provider_block(base_url, models, selected_model);
    let with_model = replace_top_level_block(&existing, "model", &model_block)?;
    let updated = replace_provider_entry(&with_model, "nan", &provider_block)?;
    write_private(&path, updated.as_bytes())?;
    configure_profile_search(profile, base_url, web_search_enabled)
}

fn configure_profile_search(
    profile: &Path,
    base_url: &str,
    enabled: bool,
) -> Result<(), HermesDesktopError> {
    let bridge_base_url = base_url
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or(base_url);
    let files = hermes_search_provider_files();
    for file in files.iter().filter(|file| file.path != "config.yaml") {
        if !enabled {
            continue;
        }
        let path = checked_profile_path(profile, &file.path)?;
        let parent = path
            .parent()
            .ok_or(HermesDesktopError::InvalidProfilePath)?;
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(HermesDesktopError::ProtectProfile)?;
        let rendered = file.content_template.replace(
            nan_harness_core::launch_plan::BRIDGE_BASE_URL_PLACEHOLDER,
            bridge_base_url,
        );
        write_private(&path, rendered.as_bytes())?;
    }

    let config_path = profile.join("config.yaml");
    let contents =
        fs::read_to_string(&config_path).map_err(HermesDesktopError::ReadProfileConfig)?;
    let mut document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&contents).map_err(HermesDesktopError::ParseProfileConfig)?;
    let original = document.clone();
    if enabled {
        let template = files
            .iter()
            .find(|file| file.path == "config.yaml")
            .ok_or(HermesDesktopError::MissingSearchTemplate)?
            .content_template
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_BEGIN, "")
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_END, "");
        let template_patch: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&template).map_err(HermesDesktopError::ParseProfileConfig)?;
        merge_yaml_value(&mut document, template_patch);
    } else {
        remove_managed_search(&mut document);
    }
    if document == original {
        return Ok(());
    }
    let rendered =
        serde_yaml_ng::to_string(&document).map_err(HermesDesktopError::SerializeProfileConfig)?;
    write_private(&config_path, rendered.as_bytes())
}

fn merge_yaml_value(base: &mut serde_yaml_ng::Value, patch: serde_yaml_ng::Value) {
    match (base, patch) {
        (serde_yaml_ng::Value::Mapping(base), serde_yaml_ng::Value::Mapping(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = base.get_mut(&key) {
                    merge_yaml_value(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (serde_yaml_ng::Value::Sequence(base), serde_yaml_ng::Value::Sequence(patch)) => {
            for value in patch {
                if !base.contains(&value) {
                    base.push(value);
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

fn remove_managed_search(document: &mut serde_yaml_ng::Value) {
    let serde_yaml_ng::Value::Mapping(root) = document else {
        return;
    };
    let plugins = serde_yaml_ng::Value::String("plugins".to_owned());
    let enabled = serde_yaml_ng::Value::String("enabled".to_owned());
    if let Some(serde_yaml_ng::Value::Mapping(plugins)) = root.get_mut(&plugins)
        && let Some(serde_yaml_ng::Value::Sequence(values)) = plugins.get_mut(&enabled)
    {
        values.retain(|value| value.as_str() != Some("web/nan_harness"));
    }
    let web = serde_yaml_ng::Value::String("web".to_owned());
    let backend = serde_yaml_ng::Value::String("search_backend".to_owned());
    if let Some(serde_yaml_ng::Value::Mapping(web)) = root.get_mut(&web)
        && web.get(&backend).and_then(serde_yaml_ng::Value::as_str) == Some("nan-harness")
    {
        web.remove(&backend);
    }
}

fn reject_profile_symlink(path: &Path) -> Result<(), HermesDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(HermesDesktopError::UnsafePluginPath)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HermesDesktopError::ReadProfileConfig(error)),
    }
}

fn checked_profile_path(profile: &Path, relative: &str) -> Result<PathBuf, HermesDesktopError> {
    let mut path = profile.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(HermesDesktopError::InvalidProfilePath);
        };
        path.push(component);
        reject_profile_symlink(&path)?;
    }
    Ok(path)
}

fn replace_top_level_block(
    source: &str,
    key: &str,
    replacement: &str,
) -> Result<String, HermesDesktopError> {
    let lines = source.lines().collect::<Vec<_>>();
    let prefix = format!("{key}:");
    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with(&prefix) {
            if line.trim() != prefix {
                return Err(HermesDesktopError::UnsupportedProfileConfig(key.to_owned()));
            }
            if start.replace(index).is_some() {
                return Err(HermesDesktopError::UnsupportedProfileConfig(key.to_owned()));
            }
            continue;
        }
        if start.is_some() && !line.is_empty() && !line.starts_with(char::is_whitespace) {
            end = index;
            break;
        }
    }
    let mut output = Vec::new();
    if let Some(start) = start {
        output.extend_from_slice(&lines[..start]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[end..]);
    } else {
        output.extend_from_slice(&lines);
        if !output.is_empty() && !output.last().is_some_and(|line| line.is_empty()) {
            output.push("");
        }
        output.extend(replacement.lines());
    }
    Ok(format!("{}\n", output.join("\n")))
}

fn replace_provider_entry(
    source: &str,
    provider: &str,
    replacement: &str,
) -> Result<String, HermesDesktopError> {
    let lines = source.lines().collect::<Vec<_>>();
    let providers_start = lines.iter().position(|line| line.starts_with("providers:"));
    let Some(providers_start) = providers_start else {
        let mut output = source.trim_end().to_owned();
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str("providers:\n");
        output.push_str(replacement);
        output.push('\n');
        return Ok(output);
    };
    if lines[providers_start] != "providers:" {
        return Err(HermesDesktopError::UnsupportedProfileConfig(
            "providers".to_owned(),
        ));
    }
    if lines
        .iter()
        .skip(providers_start + 1)
        .any(|line| line.starts_with("providers:"))
    {
        return Err(HermesDesktopError::UnsupportedProfileConfig(
            "providers".to_owned(),
        ));
    }
    let providers_end = lines
        .iter()
        .enumerate()
        .skip(providers_start + 1)
        .find(|(_, line)| !line.is_empty() && !line.starts_with(char::is_whitespace))
        .map_or(lines.len(), |(index, _)| index);
    let target = format!("  {provider}:");
    let entry_start = lines[providers_start + 1..providers_end]
        .iter()
        .position(|line| line.starts_with(&target))
        .map(|index| providers_start + 1 + index);
    let mut output = Vec::new();
    if let Some(entry_start) = entry_start {
        if lines[entry_start] != target {
            return Err(HermesDesktopError::UnsupportedProfileConfig(format!(
                "providers.{provider}"
            )));
        }
        let entry_end = lines
            .iter()
            .enumerate()
            .take(providers_end)
            .skip(entry_start + 1)
            .find(|(_, line)| {
                !line.is_empty()
                    && (line.starts_with("  ") && !line.starts_with("   ")
                        || !line.starts_with(char::is_whitespace))
            })
            .map_or(providers_end, |(index, _)| index);
        output.extend_from_slice(&lines[..entry_start]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[entry_end..]);
    } else {
        output.extend_from_slice(&lines[..providers_end]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[providers_end..]);
    }
    Ok(format!("{}\n", output.join("\n")))
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum SessionMode {
    Persistent,
    Diagnostic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileBackup {
    existed: bool,
    original_sha256: Option<String>,
    backup_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReceipt {
    schema_version: u8,
    mode: SessionMode,
    profile: PathBuf,
    active_profile: FileBackup,
    environment: FileBackup,
    active_applied_sha256: String,
    environment_applied_sha256: String,
}

fn begin_session(
    paths: &DesktopPaths,
    profile: &Path,
    mode: SessionMode,
    session_key: &str,
) -> Result<(), HermesDesktopError> {
    if paths.session_receipt.exists() {
        return Err(HermesDesktopError::PendingRecovery);
    }
    let environment_path = profile.join(".env");
    let active_original = read_optional(&paths.active_profile)?;
    let environment_original = read_optional(&environment_path)?;
    let environment_applied = add_env_block(environment_original.as_deref(), session_key)?;
    let profile_name = profile
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HermesDesktopError::InvalidProfilePath)?;
    let active_applied = serde_json::to_vec_pretty(&json!({"profile": profile_name}))
        .map_err(HermesDesktopError::Serialize)?;

    fs::create_dir_all(&paths.backup_directory)
        .map_err(HermesDesktopError::CreateBackupDirectory)?;
    restrict_path(&paths.backup_directory, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectBackupDirectory)?;
    let active_backup = backup_file(
        &paths.backup_directory,
        "active-profile.backup",
        active_original.as_deref(),
    )?;
    let environment_backup = backup_file(
        &paths.backup_directory,
        "profile-env.backup",
        environment_original.as_deref(),
    )?;
    let receipt = SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        mode,
        profile: profile.to_path_buf(),
        active_profile: active_backup,
        environment: environment_backup,
        active_applied_sha256: sha256(&active_applied),
        environment_applied_sha256: sha256(&environment_applied),
    };
    write_json_private(&paths.session_receipt, &receipt)?;
    if let Err(error) = write_private(&environment_path, &environment_applied)
        .and_then(|()| write_private(&paths.active_profile, &active_applied))
    {
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

fn add_env_block(
    original: Option<&[u8]>,
    session_key: &str,
) -> Result<Vec<u8>, HermesDesktopError> {
    let original = original.unwrap_or_default();
    let text = std::str::from_utf8(original).map_err(HermesDesktopError::ProfileEnvUtf8)?;
    if text.contains(ENV_BLOCK_BEGIN)
        || text.contains(ENV_BLOCK_END)
        || text.lines().any(defines_nan_api_key)
    {
        return Err(HermesDesktopError::ProfileCredentialConflict);
    }
    let mut output = text.trim_end_matches(['\r', '\n']).to_owned();
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(ENV_BLOCK_BEGIN);
    output.push('\n');
    output.push_str("NAN_API_KEY=");
    output.push_str(&dotenv_quote(session_key));
    output.push('\n');
    output.push_str(ENV_BLOCK_END);
    output.push('\n');
    Ok(output.into_bytes())
}

fn dotenv_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn defines_nan_api_key(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with('#') {
        return false;
    }
    line.strip_prefix("export ")
        .unwrap_or(line)
        .starts_with("NAN_API_KEY=")
}

fn restore_session(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some(receipt) = read_optional_json::<SessionReceipt>(&paths.session_receipt)? else {
        return Ok(());
    };
    if receipt.schema_version != SESSION_SCHEMA_VERSION {
        return Err(HermesDesktopError::UnsupportedSessionSchema);
    }
    validate_session_receipt(paths, &receipt)?;
    restore_active_profile(paths, &receipt)?;
    restore_environment(paths, &receipt)?;
    if receipt.mode == SessionMode::Diagnostic {
        remove_owned_diagnostic_profile(&receipt.profile)?;
    }
    remove_if_exists(&paths.backup_directory.join("active-profile.backup"))
        .map_err(HermesDesktopError::RemoveBackup)?;
    remove_if_exists(&paths.backup_directory.join("profile-env.backup"))
        .map_err(HermesDesktopError::RemoveBackup)?;
    match fs::remove_dir(&paths.backup_directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(HermesDesktopError::RemoveBackup(error)),
    }
    remove_if_exists(&paths.session_receipt).map_err(HermesDesktopError::RemoveReceipt)?;
    Ok(())
}

fn validate_session_receipt(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let valid = match receipt.mode {
        SessionMode::Persistent => receipt.profile == paths.managed_profile,
        SessionMode::Diagnostic => {
            receipt.profile.parent() == Some(paths.profiles_root.as_path())
                && receipt.profile.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with(DIAGNOSTIC_PROFILE_PREFIX)
                })
        }
    };
    if !valid
        || receipt.active_profile.backup_file != "active-profile.backup"
        || receipt.environment.backup_file != "profile-env.backup"
    {
        return Err(HermesDesktopError::InvalidRecoveryReceipt);
    }
    Ok(())
}

fn restore_active_profile(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let current = read_optional(&paths.active_profile)?;
    if file_is_original(current.as_deref(), &receipt.active_profile) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|value| sha256(value) == receipt.active_applied_sha256)
    {
        restore_backup(paths, &paths.active_profile, &receipt.active_profile)?;
    } else {
        eprintln!(
            "warning: Hermes Desktop's active profile changed during the NaN session; preserving the user's selection."
        );
    }
    Ok(())
}

fn restore_environment(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let path = receipt.profile.join(".env");
    let current = read_optional(&path)?;
    if file_is_original(current.as_deref(), &receipt.environment) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|value| sha256(value) == receipt.environment_applied_sha256)
    {
        return restore_backup(paths, &path, &receipt.environment);
    }
    let Some(current) = current else {
        return Ok(());
    };
    let cleaned = remove_env_block(&current)?;
    write_private(&path, &cleaned)
}

fn remove_env_block(contents: &[u8]) -> Result<Vec<u8>, HermesDesktopError> {
    let text = std::str::from_utf8(contents).map_err(HermesDesktopError::ProfileEnvUtf8)?;
    let Some(begin) = text.find(ENV_BLOCK_BEGIN) else {
        return if text.lines().any(defines_nan_api_key) {
            Err(HermesDesktopError::ManagedCredentialChanged)
        } else {
            Ok(contents.to_vec())
        };
    };
    let end_start = text[begin..]
        .find(ENV_BLOCK_END)
        .map(|offset| begin + offset)
        .ok_or(HermesDesktopError::ManagedCredentialChanged)?;
    if text[end_start + ENV_BLOCK_END.len()..].contains(ENV_BLOCK_END)
        || text[..begin].contains(ENV_BLOCK_BEGIN)
    {
        return Err(HermesDesktopError::ManagedCredentialChanged);
    }
    let mut end = end_start + ENV_BLOCK_END.len();
    if text.as_bytes().get(end) == Some(&b'\r') {
        end += 1;
    }
    if text.as_bytes().get(end) == Some(&b'\n') {
        end += 1;
    }
    let mut output = String::with_capacity(text.len());
    output.push_str(&text[..begin]);
    output.push_str(&text[end..]);
    Ok(output.into_bytes())
}

fn remove_owned_diagnostic_profile(profile: &Path) -> Result<(), HermesDesktopError> {
    if !profile.exists() {
        return Ok(());
    }
    let marker = read_optional_json::<OwnerMarker>(&profile.join(OWNER_MARKER_FILE))?;
    if !marker.as_ref().is_some_and(|marker| {
        marker.schema_version == OWNERSHIP_SCHEMA_VERSION && marker.owner_id == "diagnostic"
    }) {
        return Err(HermesDesktopError::DiagnosticOwnershipMismatch);
    }
    fs::remove_dir_all(profile).map_err(HermesDesktopError::RemoveProfile)
}

fn ensure_recovery_is_safe(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning);
    }
    if live_update_owner(&paths.update_marker)?.is_some() {
        return Err(HermesDesktopError::UpdateStillRunning);
    }
    Ok(())
}

fn backup_file(
    directory: &Path,
    name: &str,
    contents: Option<&[u8]>,
) -> Result<FileBackup, HermesDesktopError> {
    let path = directory.join(name);
    match contents {
        Some(contents) => write_private(&path, contents)?,
        None => remove_if_exists(&path).map_err(HermesDesktopError::RemoveBackup)?,
    }
    Ok(FileBackup {
        existed: contents.is_some(),
        original_sha256: contents.map(sha256),
        backup_file: name.to_owned(),
    })
}

fn restore_backup(
    paths: &DesktopPaths,
    target: &Path,
    backup: &FileBackup,
) -> Result<(), HermesDesktopError> {
    if backup.existed {
        let backup_path = paths.backup_directory.join(&backup.backup_file);
        let contents = fs::read(&backup_path).map_err(HermesDesktopError::ReadBackup)?;
        if Some(sha256(&contents)) != backup.original_sha256 {
            return Err(HermesDesktopError::BackupHashMismatch);
        }
        write_private(target, &contents)
    } else {
        remove_if_exists(target).map_err(HermesDesktopError::Restore)
    }
}

fn file_is_original(current: Option<&[u8]>, backup: &FileBackup) -> bool {
    match (current, backup.existed, backup.original_sha256.as_deref()) {
        (None, false, _) => true,
        (Some(current), true, Some(hash)) => sha256(current) == hash,
        _ => false,
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, HermesDesktopError> {
    reject_profile_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HermesDesktopError::ReadFile(error)),
    }
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, HermesDesktopError> {
    let Some(contents) = read_optional(path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(HermesDesktopError::ParseReceipt)
}

fn write_json_private(path: &Path, value: &impl Serialize) -> Result<(), HermesDesktopError> {
    let payload = serde_json::to_vec_pretty(value).map_err(HermesDesktopError::Serialize)?;
    write_private(path, &payload)
}

fn write_private(path: &Path, payload: &[u8]) -> Result<(), HermesDesktopError> {
    reject_profile_symlink(path)?;
    write_private_file(path, payload, None).map_err(HermesDesktopError::Persistence)
}

fn remove_if_exists(path: &Path) -> Result<(), std::io::Error> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(std::io::Error::other(
            "managed Desktop state contains an unsafe symbolic link",
        ));
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sha256(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

fn random_id() -> Result<String, HermesDesktopError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(HermesDesktopError::Random)?;
    Ok(bytes
        .iter()
        .fold(String::with_capacity(24), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        }))
}

fn append_diagnostics(target: &mut Vec<BridgeDiagnostic>, diagnostics: Vec<BridgeDiagnostic>) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum HermesDesktopError {
    #[error("Hermes Desktop is already open; close it before running `nan hermes-desktop`")]
    AlreadyRunning,
    #[error("another `nan hermes-desktop` session is active")]
    ConcurrentSession,
    #[error("Hermes Desktop is updating; wait for it to finish before retrying")]
    UpdateAlreadyRunning,
    #[error(
        "Hermes Desktop's updater is still running; wait for it to finish, then run `nan hermes-desktop --restore`"
    )]
    UpdateStillRunning,
    #[error(
        "Hermes Desktop's update exceeded 20 minutes; wait for it to finish, then run `nan hermes-desktop --restore`"
    )]
    UpdateTimedOut,
    #[error(
        "Hermes Desktop completed its update but did not relaunch; start it again with `nan hermes-desktop`"
    )]
    DidNotRelaunch,
    #[error(
        "the Hermes profile 'nan' already exists and is not managed by nan-harness; rename that profile before running `nan hermes-desktop`"
    )]
    UnmanagedNanProfile,
    #[error(
        "both active and parked managed Hermes profiles exist; preserve both directories and resolve the duplicate before retrying"
    )]
    ManagedProfileConflict,
    #[error(
        "the parked Hermes profile does not have matching nan-harness ownership; move it aside before retrying"
    )]
    ParkedProfileOwnershipMismatch,
    #[error(
        "the Hermes profile visibility guard does not match nan-harness ownership; preserve that entry and run `nan hermes-desktop --restore`"
    )]
    ProfileGuardOwnershipMismatch,
    #[error("the nan-harness ownership receipt exists but its Hermes profile is missing")]
    ManagedProfileMissing,
    #[error("the Hermes profile ownership marker does not match nan-harness state")]
    OwnershipMismatch,
    #[error("the Hermes Desktop ownership receipt schema is not supported")]
    UnsupportedOwnershipSchema,
    #[error("a previous Hermes Desktop session needs recovery; run `nan hermes-desktop --restore`")]
    PendingRecovery,
    #[error("--restore cannot be combined with launch options")]
    RestoreWithLaunchOptions,
    #[error("Hermes Desktop argument '{0}' is incompatible with a managed NaN launch")]
    UnsupportedDesktopArgument(&'static str),
    #[error(
        "Hermes Desktop requires Hermes Agent {minimum} or newer; found {detected}; update Hermes or pass --allow-unsupported"
    )]
    DesktopVersionUnsupported { detected: Version, minimum: Version },
    #[error(
        "Hermes Desktop {detected} is newer than the last compatible version {last}; pass --allow-untested to continue"
    )]
    DesktopVersionUntested { detected: Version, last: Version },
    #[error("Hermes Desktop is unavailable on this platform")]
    DesktopUnavailable,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error("the embedded Hermes Desktop compatibility evidence is incomplete")]
    InvalidCompatibilityEvidence,
    #[error("could not inspect Hermes Desktop launch capabilities: {0}")]
    CapabilityProbe(std::io::Error),
    #[error("Hermes Desktop capability probe failed with exit code {0:?}")]
    CapabilityProbeFailed(Option<i32>),
    #[error("Hermes Desktop is missing required launch capabilities: {0}")]
    MissingDesktopCapabilities(String),
    #[error(
        "model '{model}' is not available for this NaN credential; choose one of: {available:?}"
    )]
    ModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("NaN returned no conversational models")]
    EmptyModelCatalog,
    #[error("the configured stable Hermes Desktop gateway port {port} is unavailable: {source}")]
    StablePortUnavailable { port: u16, source: std::io::Error },
    #[error("could not bind a local Hermes Desktop gateway: {0}")]
    BindGateway(std::io::Error),
    #[error(transparent)]
    Gateway(#[from] ChatGatewayError),
    #[error("the Hermes Desktop gateway stopped unexpectedly")]
    GatewayExited,
    #[error("could not launch Hermes Desktop: {0}")]
    Launch(std::io::Error),
    #[error("could not wait for Hermes Desktop: {0}")]
    Wait(std::io::Error),
    #[error("could not inspect Hermes Desktop processes: {0}")]
    ProcessCheck(std::io::Error),
    #[error("Hermes Desktop process inspection failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[cfg(any(windows, test))]
    #[error("Hermes Desktop process inspection returned invalid JSON: {0}")]
    ParseProcessListing(serde_json::Error),
    #[cfg(any(windows, test))]
    #[error("Hermes Desktop process inspection omitted its process ID")]
    InvalidProcessListing,
    #[cfg(any(windows, test))]
    #[error("multiple Hermes Desktop main processes are running; close them before retrying")]
    AmbiguousDesktopProcesses,
    #[error("could not terminate Hermes Desktop: {0}")]
    Terminate(std::io::Error),
    #[error("Hermes Desktop termination failed with exit code {0:?}")]
    TerminateFailed(Option<i32>),
    #[error("Hermes Desktop did not terminate")]
    DidNotTerminate,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("NAN_HARNESS_CONFIG_DIR must be an absolute path for Hermes Desktop recovery")]
    InvalidStateDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("HERMES_HOME must be an absolute path for Hermes Desktop recovery")]
    InvalidHermesHome,
    #[error("could not create private Hermes Desktop state: {0}")]
    CreateStateDirectory(std::io::Error),
    #[error("could not protect private Hermes Desktop state: {0}")]
    ProtectStateDirectory(std::io::Error),
    #[error("could not open the Hermes Desktop session lock: {0}")]
    OpenLock(std::io::Error),
    #[error("could not protect the Hermes Desktop session lock: {0}")]
    ProtectLock(std::io::Error),
    #[error("could not lock the Hermes Desktop session: {0}")]
    Lock(std::io::Error),
    #[error("could not create the managed Hermes profile: {0}")]
    CreateProfile(std::io::Error),
    #[error("could not protect the managed Hermes profile: {0}")]
    ProtectProfile(std::io::Error),
    #[error("could not create the private parked-profile directory: {0}")]
    CreateParkingDirectory(std::io::Error),
    #[error("could not protect the private parked-profile directory: {0}")]
    ProtectParkingDirectory(std::io::Error),
    #[error("could not activate the managed Hermes profile: {0}")]
    ActivateProfile(std::io::Error),
    #[error("could not park the managed Hermes profile: {0}")]
    ParkProfile(std::io::Error),
    #[error("could not remove legacy Hermes profile metadata: {0}")]
    RemoveProfileMetadata(std::io::Error),
    #[error("could not create the Hermes profile visibility guard: {0}")]
    CreateProfileGuard(std::io::Error),
    #[error("could not write the Hermes profile visibility guard: {0}")]
    WriteProfileGuard(std::io::Error),
    #[error("could not remove the Hermes profile visibility guard: {0}")]
    RemoveProfileGuard(std::io::Error),
    #[error("could not create the private recreated-profile recovery directory: {0}")]
    CreateRecoveryDirectory(std::io::Error),
    #[error("could not protect the private recreated-profile recovery directory: {0}")]
    ProtectRecoveryDirectory(std::io::Error),
    #[error("could not preserve the recreated Hermes profile for recovery: {0}")]
    QuarantineRecreatedProfile(std::io::Error),
    #[error("could not enumerate Hermes profiles: {0}")]
    ReadProfiles(std::io::Error),
    #[error("could not remove an owned diagnostic Hermes profile: {0}")]
    RemoveProfile(std::io::Error),
    #[error("a diagnostic Hermes profile no longer has its nan-harness ownership marker")]
    DiagnosticOwnershipMismatch,
    #[error("could not read the managed Hermes profile configuration: {0}")]
    ReadProfileConfig(std::io::Error),
    #[error("the managed Hermes profile uses an unsupported YAML form for '{0}'")]
    UnsupportedProfileConfig(String),
    #[error("the managed Hermes profile configuration is invalid YAML: {0}")]
    ParseProfileConfig(serde_yaml_ng::Error),
    #[error("could not serialize the managed Hermes profile configuration: {0}")]
    SerializeProfileConfig(serde_yaml_ng::Error),
    #[error("the managed Hermes profile contains an unsafe symbolic link")]
    UnsafePluginPath,
    #[error("the shared Hermes search renderer did not provide config.yaml")]
    MissingSearchTemplate,
    #[error(
        "the managed Hermes profile already defines NAN_API_KEY; remove that entry before running `nan hermes-desktop`"
    )]
    ProfileCredentialConflict,
    #[error(
        "the managed Hermes profile credential block changed; remove the nan-harness NAN_API_KEY block, then run `nan hermes-desktop --restore`"
    )]
    ManagedCredentialChanged,
    #[error("the managed Hermes profile .env is not UTF-8: {0}")]
    ProfileEnvUtf8(std::str::Utf8Error),
    #[error("the managed Hermes profile path is invalid")]
    InvalidProfilePath,
    #[error("could not create private Hermes Desktop recovery backups: {0}")]
    CreateBackupDirectory(std::io::Error),
    #[error("could not protect private Hermes Desktop recovery backups: {0}")]
    ProtectBackupDirectory(std::io::Error),
    #[error("could not read a private Hermes Desktop recovery backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Hermes Desktop recovery backup does not match its receipt")]
    BackupHashMismatch,
    #[error("could not restore Hermes Desktop state: {0}")]
    Restore(std::io::Error),
    #[error("could not remove the Hermes Desktop recovery receipt: {0}")]
    RemoveReceipt(std::io::Error),
    #[error("could not remove private Hermes Desktop recovery backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not read Hermes Desktop's update marker: {0}")]
    ReadUpdateMarker(std::io::Error),
    #[error("could not read Hermes Desktop managed state: {0}")]
    ReadFile(std::io::Error),
    #[error("Hermes Desktop managed state is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the Hermes Desktop recovery receipt schema is not supported")]
    UnsupportedSessionSchema,
    #[error("the Hermes Desktop recovery receipt contains an unsafe path")]
    InvalidRecoveryReceipt,
    #[error("could not serialize Hermes Desktop managed state: {0}")]
    Serialize(serde_json::Error),
    #[error("could not generate a private Hermes Desktop identifier: {0}")]
    Random(getrandom::Error),
    #[error("could not access the NaN credential: {0}")]
    Secret(nan_harness_core::SecretError),
    #[error(transparent)]
    Persistence(crate::commands::persistence::PersistenceError),
}

impl HermesDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::UpdateAlreadyRunning
            | Self::UnmanagedNanProfile
            | Self::ManagedProfileConflict
            | Self::ParkedProfileOwnershipMismatch
            | Self::ProfileGuardOwnershipMismatch
            | Self::ManagedProfileMissing
            | Self::OwnershipMismatch
            | Self::PendingRecovery
            | Self::RestoreWithLaunchOptions
            | Self::UnsupportedDesktopArgument(_)
            | Self::DesktopVersionUnsupported { .. }
            | Self::DesktopVersionUntested { .. }
            | Self::DesktopUnavailable
            | Self::MissingDesktopCapabilities(_)
            | Self::InvalidStateDirectory
            | Self::InvalidHermesHome
            | Self::ProfileCredentialConflict
            | Self::ManagedCredentialChanged
            | Self::DiagnosticOwnershipMismatch => "NH-HERMES-DESKTOP-002",
            Self::ModelUnavailable { .. } | Self::EmptyModelCatalog => "NH-HERMES-DESKTOP-003",
            Self::UpdateStillRunning
            | Self::UpdateTimedOut
            | Self::DidNotRelaunch
            | Self::ReadUpdateMarker(_) => "NH-HERMES-DESKTOP-004",
            _ => "NH-HERMES-DESKTOP-001",
        }
    }

    // Keep this match exhaustive so every new recovery error receives a typed diagnostic.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::UpdateAlreadyRunning
            | Self::UnmanagedNanProfile
            | Self::ManagedProfileConflict
            | Self::ParkedProfileOwnershipMismatch
            | Self::ProfileGuardOwnershipMismatch
            | Self::ManagedProfileMissing
            | Self::OwnershipMismatch
            | Self::PendingRecovery
            | Self::RestoreWithLaunchOptions
            | Self::UnsupportedDesktopArgument(_)
            | Self::ProfileCredentialConflict
            | Self::ManagedCredentialChanged
            | Self::DiagnosticOwnershipMismatch
            | Self::UnsafePluginPath => {
                Diagnostic::general(DiagnosticReason::ConfigurationConflict)
            }
            Self::ModelUnavailable { .. } => {
                Diagnostic::general(DiagnosticReason::ModelUnavailable)
            }
            Self::EmptyModelCatalog => Diagnostic::general(DiagnosticReason::ModelCatalogEmpty),
            Self::Gateway(error) => gateway_diagnostic(error),
            Self::Secret(_) => Diagnostic::general(DiagnosticReason::SecretResolutionFailed),
            Self::Random(_) => Diagnostic::general(DiagnosticReason::RandomGenerationFailed),
            Self::GatewayExited => Diagnostic::general(DiagnosticReason::BridgeExited),
            Self::StablePortUnavailable { source, .. } | Self::BindGateway(source) => {
                io_diagnostic(DiagnosticOperation::BindBridge, source)
            }
            Self::Launch(source) => io_diagnostic(DiagnosticOperation::StartHarness, source),
            Self::CapabilityProbe(source) => {
                io_diagnostic(DiagnosticOperation::RunVersionCommand, source)
            }
            Self::Wait(source) | Self::ProcessCheck(source) => {
                io_diagnostic(DiagnosticOperation::WaitForHarness, source)
            }
            Self::Terminate(source) => io_diagnostic(DiagnosticOperation::StopHarness, source),
            Self::UpdateStillRunning
            | Self::UpdateTimedOut
            | Self::DidNotRelaunch
            | Self::ReadUpdateMarker(_)
            | Self::CapabilityProbeFailed(_)
            | Self::ProcessCheckFailed(_)
            | Self::TerminateFailed(_)
            | Self::DidNotTerminate => Diagnostic::general(DiagnosticReason::ProcessWaitFailed),
            #[cfg(any(windows, test))]
            Self::ParseProcessListing(_)
            | Self::InvalidProcessListing
            | Self::AmbiguousDesktopProcesses => {
                Diagnostic::general(DiagnosticReason::InvalidResponse)
            }
            Self::DesktopVersionUnsupported { .. }
            | Self::DesktopVersionUntested { .. }
            | Self::DesktopUnavailable
            | Self::UnsupportedOwnershipSchema
            | Self::UnsupportedSessionSchema => {
                Diagnostic::general(DiagnosticReason::UnsupportedVersion)
            }
            Self::UnsupportedProfileConfig(_)
            | Self::ParseProfileConfig(_)
            | Self::SerializeProfileConfig(_)
            | Self::InvalidCompatibilityEvidence
            | Self::MissingSearchTemplate
            | Self::MissingDesktopCapabilities(_)
            | Self::ProfileEnvUtf8(_)
            | Self::InvalidProfilePath
            | Self::InvalidStateDirectory
            | Self::InvalidHermesHome
            | Self::ParseReceipt(_)
            | Self::InvalidRecoveryReceipt
            | Self::BackupHashMismatch => {
                Diagnostic::general(DiagnosticReason::InvalidConfiguration)
            }
            Self::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
            Self::MissingStateDirectory | Self::MissingHomeDirectory => {
                Diagnostic::general(DiagnosticReason::MissingDirectory)
            }
            Self::CreateStateDirectory(source)
            | Self::ProtectStateDirectory(source)
            | Self::OpenLock(source)
            | Self::ProtectLock(source)
            | Self::Lock(source)
            | Self::CreateProfile(source)
            | Self::ProtectProfile(source)
            | Self::CreateParkingDirectory(source)
            | Self::ProtectParkingDirectory(source)
            | Self::ActivateProfile(source)
            | Self::ParkProfile(source)
            | Self::RemoveProfileMetadata(source)
            | Self::CreateProfileGuard(source)
            | Self::WriteProfileGuard(source)
            | Self::RemoveProfileGuard(source)
            | Self::CreateRecoveryDirectory(source)
            | Self::ProtectRecoveryDirectory(source)
            | Self::QuarantineRecreatedProfile(source)
            | Self::ReadProfiles(source)
            | Self::RemoveProfile(source)
            | Self::ReadProfileConfig(source)
            | Self::CreateBackupDirectory(source)
            | Self::ProtectBackupDirectory(source)
            | Self::ReadBackup(source)
            | Self::Restore(source)
            | Self::RemoveReceipt(source)
            | Self::RemoveBackup(source)
            | Self::ReadFile(source) => {
                io_diagnostic(DiagnosticOperation::WriteConfiguration, source)
            }
            Self::Persistence(_) | Self::Compatibility(_) => {
                Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
            }
        }
    }
}

fn gateway_diagnostic(error: &ChatGatewayError) -> Diagnostic {
    match error {
        ChatGatewayError::Bridge(nan_harness_runtime::BridgeError::SelectedModelUnavailable {
            ..
        }) => Diagnostic::general(DiagnosticReason::ModelUnavailable),
        ChatGatewayError::Bridge(nan_harness_runtime::BridgeError::NoCompatibleModels) => {
            Diagnostic::general(DiagnosticReason::ModelCatalogEmpty)
        }
        ChatGatewayError::Bridge(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
        ChatGatewayError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        ChatGatewayError::Random(_) => {
            Diagnostic::general(DiagnosticReason::RandomGenerationFailed)
        }
    }
}

fn io_diagnostic(operation: DiagnosticOperation, source: &std::io::Error) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: IoErrorKind::from_std(source.kind()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, DesktopPaths) {
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DesktopPaths::for_test(root.path());
        (root, paths)
    }

    #[test]
    fn removing_an_absent_profile_does_not_inspect_host_processes() {
        let (_root, paths) = paths();

        let removed = remove_persistent_profile_at(&paths, || {
            panic!("the process table must not be inspected without managed Hermes state")
        })
        .expect("an absent profile should be a no-op");

        assert!(!removed);
        assert!(!paths.state_directory.exists());
        assert!(!paths.hermes_home.exists());
    }

    #[test]
    fn a_running_desktop_preserves_an_owned_profile() {
        let (_root, paths) = paths();
        create_managed_profile(&paths).expect("managed profile");
        let receipt = fs::read(&paths.ownership_receipt).expect("ownership receipt");
        let marker = fs::read(paths.parked_profile.join(OWNER_MARKER_FILE))
            .expect("profile ownership marker");

        let error = remove_persistent_profile_at(&paths, || {
            Ok(Some(DesktopProcess {
                pid: 42,
                started: "test process".to_owned(),
            }))
        })
        .expect_err("a running desktop should block profile removal");

        assert!(matches!(error, HermesDesktopError::AlreadyRunning));
        assert_eq!(
            fs::read(&paths.ownership_receipt).expect("preserved ownership receipt"),
            receipt
        );
        assert_eq!(
            fs::read(paths.parked_profile.join(OWNER_MARKER_FILE))
                .expect("preserved profile ownership marker"),
            marker
        );
    }

    #[test]
    fn desktop_version_requires_0206_unless_overridden() {
        assert!(validate_desktop_version("Hermes 0.20.6", false, false).is_ok());
        assert!(validate_desktop_version("hermes 0.21.0+desktop", false, false).is_ok());
        assert!(matches!(
            validate_desktop_version("Hermes 0.20.5", false, false),
            Err(HermesDesktopError::DesktopVersionUnsupported { .. })
        ));
        assert!(validate_desktop_version("Hermes 0.20.5", true, false).is_ok());
    }

    #[test]
    fn desktop_capability_probe_requires_managed_launch_options() {
        assert!(missing_desktop_capabilities("--source --skip-build --cwd").is_empty());
        assert_eq!(
            missing_desktop_capabilities("--source --cwd"),
            vec!["--skip-build"]
        );
    }

    #[test]
    fn managed_launch_rejects_native_one_shot_desktop_options() {
        assert_eq!(
            unsupported_desktop_argument(&["--build-only".to_owned()]),
            Some("--build-only")
        );
        assert_eq!(
            unsupported_desktop_argument(&["--setup-tcc-identity".to_owned()]),
            Some("--setup-tcc-identity")
        );
        assert_eq!(unsupported_desktop_argument(&["--source".to_owned()]), None);
    }

    #[test]
    fn alternate_hermes_root_disables_automatic_skip_build() {
        let (root, paths) = paths();
        fs::create_dir_all(paths.install_root.join("apps/desktop/release/mac-arm64"))
            .expect("macOS release directory");
        let packaged = packaged_desktop_candidates(&paths.install_root)
            .into_iter()
            .next()
            .expect("packaged desktop candidate");
        fs::create_dir_all(packaged.parent().expect("packaged desktop parent"))
            .expect("packaged desktop parent");
        fs::write(&packaged, b"desktop").expect("packaged desktop");

        assert_eq!(
            desktop_arguments(&paths, &[]),
            vec!["desktop", "--skip-build"]
        );
        assert_eq!(
            desktop_arguments(
                &paths,
                &[
                    "--hermes-root".to_owned(),
                    root.path().display().to_string()
                ]
            ),
            vec![
                "desktop".to_owned(),
                "--hermes-root".to_owned(),
                root.path().display().to_string()
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_packaged_candidates_cover_both_architectures_and_binary_names() {
        let root = Path::new("/opt/hermes");

        assert_eq!(
            packaged_desktop_candidates(root),
            [
                "apps/desktop/release/linux-unpacked/hermes",
                "apps/desktop/release/linux-unpacked/Hermes",
                "apps/desktop/release/linux-arm64-unpacked/hermes",
                "apps/desktop/release/linux-arm64-unpacked/Hermes",
            ]
            .map(|candidate| root.join(candidate))
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_process_classification_ignores_electron_helpers() {
        assert!(desktop_main_command(
            "/opt/hermes/apps/desktop/release/linux-unpacked/Hermes"
        ));
        assert!(desktop_main_command(
            "/opt/hermes/apps/desktop/node_modules/electron/dist/electron /opt/hermes/apps/desktop"
        ));
        assert!(!desktop_main_command(
            "/opt/hermes/apps/desktop/release/linux-unpacked/Hermes --type=renderer"
        ));
    }

    #[test]
    fn windows_process_classification_ignores_electron_helpers() {
        assert!(windows_desktop_main_process(
            "Hermes.exe",
            r"C:\Hermes\Hermes.exe"
        ));
        assert!(windows_desktop_main_process(
            "electron.exe",
            r"C:\repo\apps\desktop\node_modules\electron\electron.exe C:\repo\apps\desktop"
        ));
        assert!(!windows_desktop_main_process(
            "Hermes.exe",
            r"C:\Hermes\Hermes.exe --type=renderer"
        ));
    }

    #[test]
    fn windows_process_listing_selects_only_the_main_process() {
        let listing = r#"[
            {"ProcessId": 41, "CreationDate": "renderer", "Name": "Hermes.exe", "CommandLine": "Hermes.exe --type=renderer"},
            {"ProcessId": 42, "CreationDate": "main", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"}
        ]"#;

        assert_eq!(
            parse_windows_process_listing(listing).expect("valid process listing"),
            Some(DesktopProcess {
                pid: 42,
                started: "main".to_owned()
            })
        );
    }

    #[test]
    fn windows_process_listing_fails_closed_when_multiple_mains_exist() {
        let listing = r#"[
            {"ProcessId": 42, "CreationDate": "one", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"},
            {"ProcessId": 43, "CreationDate": "two", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"}
        ]"#;

        assert!(matches!(
            parse_windows_process_listing(listing),
            Err(HermesDesktopError::AmbiguousDesktopProcesses)
        ));
    }

    #[test]
    fn windows_hermes_home_prefers_user_scope_then_modern_then_legacy() {
        let root = tempfile::tempdir().expect("temporary root");
        let home = root.path().join("user");
        let local_app_data = home.join("AppData/Local");
        let modern = local_app_data.join("hermes");
        let legacy = home.join(".hermes");
        let user_scoped = root.path().join("custom-hermes");

        assert_eq!(
            choose_windows_hermes_home(
                &home,
                Some(user_scoped.clone()),
                Some(local_app_data.clone())
            ),
            user_scoped
        );
        fs::create_dir_all(&legacy).expect("legacy Hermes home");
        assert_eq!(
            choose_windows_hermes_home(&home, None, Some(local_app_data.clone())),
            legacy
        );
        fs::create_dir_all(&modern).expect("modern Hermes home");
        assert_eq!(
            choose_windows_hermes_home(&home, None, Some(local_app_data)),
            modern
        );
    }

    #[test]
    fn profile_config_preserves_unrelated_settings_and_provider_entries() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile directory");
        fs::write(
            paths.managed_profile.join("config.yaml"),
            "theme: dark\nmodel:\n  default: old\n  context_length: 12\n# keep this provider comment\nproviders:\n  other:\n    base_url: https://example.test/v1\n  nan:\n    base_url: http://old.test/v1\n  inline: {base_url: https://inline.example.test/v1}\ntools:\n  enabled: true\n",
        )
        .expect("original config");
        let models = vec![CodingModelProfile::generic("qwen3.6")];

        write_profile_config(
            &paths.managed_profile,
            "http://127.0.0.1:4321/v1",
            &models,
            "qwen3.6",
            false,
        )
        .expect("config update");

        let updated =
            fs::read_to_string(paths.managed_profile.join("config.yaml")).expect("updated config");
        assert!(updated.contains("theme: dark"));
        assert!(updated.contains("tools:\n  enabled: true"));
        assert!(updated.contains("# keep this provider comment"));
        assert!(updated.contains("  other:\n    base_url: https://example.test/v1"));
        assert!(updated.contains("  inline: {base_url: https://inline.example.test/v1}"));
        assert!(updated.contains("base_url: \"http://127.0.0.1:4321/v1\""));
        assert!(!updated.contains("http://old.test"));
    }

    #[test]
    fn profile_search_reuses_the_adapter_renderer_and_disables_only_owned_settings() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile directory");
        fs::write(
            paths.managed_profile.join("config.yaml"),
            "theme: dark\nweb:\n  user_setting: kept\n",
        )
        .expect("original config");
        let models = vec![CodingModelProfile::generic("qwen3.6")];

        write_profile_config(
            &paths.managed_profile,
            "http://127.0.0.1:4321/v1",
            &models,
            "qwen3.6",
            true,
        )
        .expect("search-enabled config update");

        let provider = fs::read_to_string(
            paths
                .managed_profile
                .join("plugins/web/nan_harness/provider.py"),
        )
        .expect("shared provider renderer should be installed");
        assert!(provider.contains("http://127.0.0.1:4321/v1/search"));
        let enabled: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &fs::read_to_string(paths.managed_profile.join("config.yaml")).expect("enabled config"),
        )
        .expect("enabled YAML");
        assert_eq!(enabled["web"]["search_backend"], "nan-harness");
        assert_eq!(enabled["web"]["user_setting"], "kept");

        write_profile_config(
            &paths.managed_profile,
            "http://127.0.0.1:4321/v1",
            &models,
            "qwen3.6",
            false,
        )
        .expect("search-disabled config update");
        let disabled: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &fs::read_to_string(paths.managed_profile.join("config.yaml"))
                .expect("disabled config"),
        )
        .expect("disabled YAML");
        assert!(disabled["web"].get("search_backend").is_none());
        assert_eq!(disabled["web"]["user_setting"], "kept");
    }

    #[test]
    fn unmanaged_nan_profile_is_never_adopted() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("existing profile");
        fs::write(paths.managed_profile.join("config.yaml"), "user: true\n").expect("user config");

        let error = ensure_managed_profile(&paths).expect_err("profile should conflict");

        assert!(matches!(error, HermesDesktopError::UnmanagedNanProfile));
        assert_eq!(
            fs::read_to_string(paths.managed_profile.join("config.yaml"))
                .expect("user config preserved"),
            "user: true\n"
        );
    }

    #[test]
    fn owned_profile_survives_nan_harness_state_removal() {
        let (_root, paths) = paths();
        let original = create_managed_profile(&paths).expect("managed profile");
        fs::remove_file(&paths.ownership_receipt).expect("simulate removed application state");

        let recovered = ensure_managed_profile(&paths).expect("owned marker should be recoverable");

        assert_eq!(recovered.owner_id, original.owner_id);
        assert_eq!(recovered.gateway_port, None);
        assert_eq!(
            profile_path_kind(&paths.managed_profile).expect("guard kind"),
            ProfilePathKind::RegularFile
        );
        assert!(paths.parked_profile.exists());
    }

    #[test]
    fn managed_profile_is_parked_between_sessions_without_losing_state() {
        let (_root, paths) = paths();
        let ownership = create_managed_profile(&paths).expect("managed profile");
        fs::write(paths.parked_profile.join("state.db"), b"persistent state")
            .expect("persistent state");
        fs::create_dir(paths.parked_profile.join("sessions")).expect("sessions directory");
        fs::write(
            paths.parked_profile.join("sessions/conversation.json"),
            b"persistent session",
        )
        .expect("persistent session");

        activate_managed_profile(&paths, &ownership).expect("activate profile");

        assert!(paths.managed_profile.exists());
        assert!(!paths.parked_profile.exists());
        assert_eq!(
            fs::read(paths.managed_profile.join("state.db")).expect("active state"),
            b"persistent state"
        );
        fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
            .expect("active parent");
        fs::write(&paths.active_profile, b"{\"profile\":\"nan\"}\n")
            .expect("managed active selection");

        park_managed_profile(&paths).expect("park profile");

        assert_eq!(
            profile_path_kind(&paths.managed_profile).expect("guard kind"),
            ProfilePathKind::RegularFile
        );
        assert!(paths.parked_profile.exists());
        assert_eq!(
            fs::read(paths.parked_profile.join("state.db")).expect("parked state"),
            b"persistent state"
        );
        assert_eq!(
            fs::read(paths.parked_profile.join("sessions/conversation.json"))
                .expect("parked session"),
            b"persistent session"
        );
        assert_eq!(
            read_optional_json::<serde_json::Value>(&paths.active_profile)
                .expect("active selection read")
                .expect("active selection"),
            json!({"profile": "default"})
        );

        activate_managed_profile(&paths, &ownership).expect("reactivate profile");
        fs::write(&paths.active_profile, b"{\"profile\":\"work\"}\n")
            .expect("user active selection");
        park_managed_profile(&paths).expect("repark profile");

        assert_eq!(
            fs::read(&paths.active_profile).expect("user selection preserved"),
            b"{\"profile\":\"work\"}\n"
        );
    }

    #[test]
    fn duplicate_active_and_parked_profiles_are_left_untouched() {
        let (_root, paths) = paths();
        create_managed_profile(&paths).expect("parked managed profile");
        fs::remove_file(&paths.managed_profile).expect("remove visibility guard");
        fs::create_dir(&paths.managed_profile).expect("duplicate active profile");
        fs::write(paths.managed_profile.join("active.txt"), b"active").expect("active sentinel");
        fs::write(paths.parked_profile.join("parked.txt"), b"parked").expect("parked sentinel");

        let error = ensure_managed_profile(&paths).expect_err("duplicate should conflict");

        assert!(matches!(error, HermesDesktopError::ManagedProfileConflict));
        assert_eq!(
            fs::read(paths.managed_profile.join("active.txt")).expect("active preserved"),
            b"active"
        );
        assert_eq!(
            fs::read(paths.parked_profile.join("parked.txt")).expect("parked preserved"),
            b"parked"
        );
    }

    #[test]
    fn visibility_guard_blocks_cached_desktop_profile_recreation() {
        let (_root, paths) = paths();
        let ownership = create_managed_profile(&paths).expect("parked managed profile");

        let guard = read_optional_json::<OwnerMarker>(&paths.managed_profile)
            .expect("guard read")
            .expect("guard");
        let recreate = fs::create_dir_all(&paths.managed_profile)
            .expect_err("a cached Desktop backend must not recreate the profile");

        assert_eq!(recreate.kind(), ErrorKind::AlreadyExists);
        assert_eq!(guard.owner_id, ownership.owner_id);
        assert!(paths.parked_profile.exists());
    }

    #[test]
    fn restore_quarantines_an_empty_recreated_profile_without_deleting_it() {
        let (_root, paths) = paths();
        create_managed_profile(&paths).expect("parked managed profile");
        fs::remove_file(&paths.managed_profile).expect("simulate legacy parking");
        fs::create_dir(&paths.managed_profile).expect("recreated profile");
        fs::write(paths.managed_profile.join("state.db"), b"recreated state")
            .expect("recreated state");

        quarantine_recreated_profile_for_restore(&paths).expect("quarantine recreated profile");

        assert_eq!(
            profile_path_kind(&paths.managed_profile).expect("guard kind"),
            ProfilePathKind::RegularFile
        );
        assert!(paths.parked_profile.exists());
        let recovered = fs::read_dir(&paths.recovered_profiles_root)
            .expect("recovery directory")
            .map(|entry| entry.expect("recovery entry").path())
            .collect::<Vec<_>>();
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            fs::read(recovered[0].join("state.db")).expect("recreated state preserved"),
            b"recreated state"
        );
    }

    #[test]
    fn restore_does_not_quarantine_a_configured_duplicate_profile() {
        let (_root, paths) = paths();
        create_managed_profile(&paths).expect("parked managed profile");
        fs::remove_file(&paths.managed_profile).expect("remove visibility guard");
        fs::create_dir(&paths.managed_profile).expect("user profile");
        fs::write(paths.managed_profile.join("config.yaml"), "user: true\n").expect("user config");

        quarantine_recreated_profile_for_restore(&paths).expect("safe recovery check");

        assert!(paths.managed_profile.is_dir());
        assert!(!paths.recovered_profiles_root.exists());
    }

    #[test]
    fn tampered_visibility_guard_is_never_replaced() {
        let (_root, paths) = paths();
        create_managed_profile(&paths).expect("parked managed profile");
        fs::write(&paths.managed_profile, b"not the owned guard").expect("tamper visibility guard");

        let error = ensure_managed_profile(&paths).expect_err("tampered guard should conflict");

        assert!(matches!(
            error,
            HermesDesktopError::ProfileGuardOwnershipMismatch
        ));
        assert_eq!(
            fs::read(&paths.managed_profile).expect("guard preserved"),
            b"not the owned guard"
        );
    }

    #[test]
    fn unowned_parked_profile_is_never_adopted() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.parked_profile).expect("existing parked profile");
        fs::write(paths.parked_profile.join("config.yaml"), "user: true\n").expect("user config");

        let error = ensure_managed_profile(&paths).expect_err("profile should conflict");

        assert!(matches!(
            error,
            HermesDesktopError::ParkedProfileOwnershipMismatch
        ));
        assert_eq!(
            fs::read_to_string(paths.parked_profile.join("config.yaml"))
                .expect("user config preserved"),
            "user: true\n"
        );
    }

    #[test]
    fn legacy_display_name_is_removed_only_when_unmodified() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        let metadata = paths.managed_profile.join("profile.yaml");
        fs::write(&metadata, "display_name: NaN\n").expect("legacy metadata");

        remove_legacy_profile_display_name(&paths.managed_profile).expect("remove legacy metadata");

        assert!(!metadata.exists());

        fs::write(&metadata, "display_name: NaN\ndescription: keep\n").expect("custom metadata");
        remove_legacy_profile_display_name(&paths.managed_profile)
            .expect("preserve customized metadata");

        assert_eq!(
            fs::read_to_string(&metadata).expect("custom metadata preserved"),
            "display_name: NaN\ndescription: keep\n"
        );
    }

    #[test]
    fn normal_restore_removes_only_the_launch_scoped_credential() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
            .expect("active parent");
        fs::write(paths.managed_profile.join(".env"), "USER_SETTING=before\n")
            .expect("original env");
        fs::write(&paths.active_profile, b"{\"profile\":\"work\"}\n").expect("original active");

        begin_session(
            &paths,
            &paths.managed_profile,
            SessionMode::Persistent,
            "session-secret",
        )
        .expect("session setup");
        let active_env =
            fs::read_to_string(paths.managed_profile.join(".env")).expect("active env");
        assert!(active_env.contains("session-secret"));
        restore_session(&paths).expect("restore");

        assert_eq!(
            fs::read_to_string(paths.managed_profile.join(".env")).expect("restored env"),
            "USER_SETTING=before\n"
        );
        assert_eq!(
            fs::read(&paths.active_profile).expect("restored active"),
            b"{\"profile\":\"work\"}\n"
        );
        assert!(!paths.session_receipt.exists());
    }

    #[test]
    fn restore_preserves_a_user_profile_switch() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        begin_session(
            &paths,
            &paths.managed_profile,
            SessionMode::Persistent,
            "session-secret",
        )
        .expect("session setup");
        fs::write(&paths.active_profile, b"{\"profile\":\"user-choice\"}\n").expect("user switch");

        restore_session(&paths).expect("restore");

        assert_eq!(
            fs::read(&paths.active_profile).expect("preserved active"),
            b"{\"profile\":\"user-choice\"}\n"
        );
        assert!(
            !fs::read_to_string(paths.managed_profile.join(".env"))
                .unwrap_or_default()
                .contains("session-secret")
        );
    }

    #[test]
    fn receipt_never_contains_the_session_secret() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        begin_session(
            &paths,
            &paths.managed_profile,
            SessionMode::Persistent,
            "do-not-copy-this-secret",
        )
        .expect("session setup");

        let receipt = fs::read_to_string(&paths.session_receipt).expect("receipt");
        assert!(!receipt.contains("do-not-copy-this-secret"));
    }

    #[test]
    fn restore_accepts_user_changes_after_the_session_credential_was_removed() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        begin_session(
            &paths,
            &paths.managed_profile,
            SessionMode::Persistent,
            "session-secret",
        )
        .expect("session setup");
        fs::write(paths.managed_profile.join(".env"), "USER_SETTING=changed\n")
            .expect("safe user edit");

        restore_session(&paths).expect("credential-free user edit is safe");

        assert_eq!(
            fs::read_to_string(paths.managed_profile.join(".env")).expect("preserved env"),
            "USER_SETTING=changed\n"
        );
        assert!(!paths.session_receipt.exists());
    }

    #[test]
    fn stable_port_is_reused_from_owned_state() {
        let (_root, paths) = paths();
        let mut ownership = create_managed_profile(&paths).expect("managed profile");
        ownership.gateway_port = Some(43127);
        write_json_private(&paths.ownership_receipt, &ownership).expect("ownership update");

        let loaded = ensure_managed_profile(&paths).expect("owned profile");

        assert_eq!(loaded.gateway_port, Some(43127));
    }

    #[tokio::test]
    async fn second_interrupt_during_update_preserves_recovery_state() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.hermes_home).expect("Hermes home");
        fs::write(
            &paths.update_marker,
            format!("{}\nstarted\n", std::process::id()),
        )
        .expect("live update marker");
        let (sender, mut signals) = tokio::sync::mpsc::unbounded_channel();
        sender.send(130).expect("first interrupt");
        sender.send(130).expect("second interrupt");
        let mut gateway = None;

        let result = wait_for_update(&paths, &mut gateway, &mut signals)
            .await
            .expect("update wait");

        assert_eq!(result, UpdateWaitCompletion::PreserveRecovery(130));
        assert!(paths.update_marker.exists());
    }

    #[test]
    fn interrupt_protection_carries_across_update_and_relaunch() {
        let mut interrupt_seen = false;

        assert!(!update_interrupt_requests_exit(130, &mut interrupt_seen));
        assert!(interrupt_seen);
        assert!(update_interrupt_requests_exit(130, &mut interrupt_seen));
    }

    #[test]
    fn desktop_relaunch_resets_the_termination_quiescence_window() {
        let start = Instant::now();
        let mut quiet_since = None;

        assert!(!desktop_quiescence_reached(
            &mut quiet_since,
            start,
            false,
            DESKTOP_QUIESCENCE_INTERVAL,
        ));
        assert!(!desktop_quiescence_reached(
            &mut quiet_since,
            start + Duration::from_secs(4),
            false,
            DESKTOP_QUIESCENCE_INTERVAL,
        ));
        assert!(!desktop_quiescence_reached(
            &mut quiet_since,
            start + Duration::from_secs(4),
            true,
            DESKTOP_QUIESCENCE_INTERVAL,
        ));
        assert!(!desktop_quiescence_reached(
            &mut quiet_since,
            start + Duration::from_secs(7),
            false,
            DESKTOP_QUIESCENCE_INTERVAL,
        ));
        assert!(desktop_quiescence_reached(
            &mut quiet_since,
            start + Duration::from_secs(12),
            false,
            DESKTOP_QUIESCENCE_INTERVAL,
        ));
    }

    #[test]
    fn restore_is_idempotent_after_files_were_restored_before_receipt_cleanup() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.managed_profile).expect("profile");
        fs::write(paths.managed_profile.join(".env"), "USER_SETTING=before\n")
            .expect("original env");
        begin_session(
            &paths,
            &paths.managed_profile,
            SessionMode::Persistent,
            "session-secret",
        )
        .expect("session setup");
        let receipt = read_optional_json::<SessionReceipt>(&paths.session_receipt)
            .expect("receipt read")
            .expect("receipt");
        restore_active_profile(&paths, &receipt).expect("active profile restore");
        restore_environment(&paths, &receipt).expect("environment restore");
        remove_if_exists(&paths.backup_directory.join("active-profile.backup"))
            .expect("active backup cleanup");
        remove_if_exists(&paths.backup_directory.join("profile-env.backup"))
            .expect("environment backup cleanup");

        restore_session(&paths).expect("repeated recovery should finish");

        assert!(!paths.session_receipt.exists());
        assert_eq!(
            fs::read_to_string(paths.managed_profile.join(".env")).expect("restored env"),
            "USER_SETTING=before\n"
        );
    }
}
