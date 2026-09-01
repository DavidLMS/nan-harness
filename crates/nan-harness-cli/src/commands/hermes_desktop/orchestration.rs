use super::*;

pub(super) fn restore_command(paths: &DesktopPaths) -> Result<i32, CliError> {
    let _lock = SessionLock::acquire(paths)?;
    ensure_recovery_is_safe(paths)?;
    restore_session(paths)?;
    quarantine_recreated_profile_for_restore(paths)?;
    park_managed_profile_if_owned(paths)?;
    cleanup_stale_diagnostic_profiles(paths)?;
    eprintln!("Hermes Desktop managed launch state restored and its NaN profile parked.");
    Ok(0)
}

pub(super) fn print_dry_run(
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

pub(super) async fn run_desktop_session(
    arguments: &HermesDesktopArgs,
    interactive: bool,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
    paths: &DesktopPaths,
) -> Result<i32, CliError> {
    let _lock = SessionLock::acquire(paths)?;
    prepare_session_state(paths)?;
    let Some(prepared) = prepare_desktop_launch(arguments, interactive, paths).await? else {
        return Ok(0);
    };
    let PreparedDesktopLaunch {
        discovery,
        launch_arguments,
        manager,
        selected_model,
        mut gateway,
    } = prepared;

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
            manager.save_last_desktop_selection(DesktopHarnessKind::Hermes, &selected_model)
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

struct PreparedDesktopLaunch {
    discovery: DiscoveryReport,
    launch_arguments: Vec<String>,
    manager: PersistenceManager,
    selected_model: String,
    gateway: Option<RunningChatCompletionsGateway>,
}

pub(super) fn prepare_session_state(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning);
    }
    if live_update_owner(&paths.update_marker)?.is_some() {
        return Err(HermesDesktopError::UpdateAlreadyRunning);
    }
    if paths.session_receipt.exists() {
        restore_session(paths)?;
    }
    park_managed_profile_if_owned(paths)?;
    cleanup_stale_diagnostic_profiles(paths)
}

async fn prepare_desktop_launch(
    arguments: &HermesDesktopArgs,
    interactive: bool,
    paths: &DesktopPaths,
) -> Result<Option<PreparedDesktopLaunch>, CliError> {
    let Some(discovery) = discover_or_install_harness(HarnessKind::Hermes, &arguments.run)? else {
        return Ok(None);
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
    )?
    .to_owned();
    let gateway = prepare_profile_session(
        arguments.no_chat_gateway,
        paths,
        &config.config,
        &models,
        &selected_model,
        !arguments.run.search.no_search,
    )
    .await?;
    Ok(Some(PreparedDesktopLaunch {
        discovery,
        launch_arguments: desktop_arguments(paths, &arguments.run.arguments),
        manager,
        selected_model,
        gateway,
    }))
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
