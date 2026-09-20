use super::*;
use crate::app::{ConfigArgs, ConfigTarget};
use crate::commands::credentials;
use crate::commands::pen_desktop::{self, PenDesktopError};
use nan_harness_core::{MediaSelection, WebSearchPolicy};
use std::io::{BufRead as _, Write as _};

pub(crate) async fn run(
    arguments: &ConfigArgs,
    interactive: bool,
) -> Result<(), ConfigurationError> {
    validate_arguments(arguments)?;
    let manager = ConfigurationManager::from_environment()?;
    if arguments.harness == Some(ConfigTarget::Pen) {
        return run_pen(arguments, interactive).await;
    }
    let harness = arguments.harness.and_then(ConfigTarget::stable);

    if let Some(harness) = bridge_only_harness(harness) {
        run_bridge_only(harness);
        return Ok(());
    }
    if arguments.status {
        return run_status(&manager, harness);
    }
    if arguments.remove_all {
        return run_remove_all(&manager, arguments.yes, interactive);
    }
    if arguments.remove {
        return run_remove(&manager, harness);
    }
    if arguments.refresh_all {
        return run_refresh_all(&manager, interactive).await;
    }

    configure_harness(&manager, arguments, interactive).await
}

fn bridge_only_harness(harness: Option<HarnessKind>) -> Option<HarnessKind> {
    harness.filter(|harness| !SUPPORTED_HARNESSES.contains(harness))
}

fn run_bridge_only(harness: HarnessKind) {
    print_bridge_only(harness);
}

fn run_status(
    manager: &ConfigurationManager,
    harness: Option<HarnessKind>,
) -> Result<(), ConfigurationError> {
    if let Some(harness) = harness {
        print_status(manager, harness)
    } else {
        print_all_statuses(manager)
    }
}

fn run_remove_all(
    manager: &ConfigurationManager,
    yes: bool,
    interactive: bool,
) -> Result<(), ConfigurationError> {
    if !confirm_remove_all(manager, yes, interactive)? {
        println!(
            "{}",
            nan_harness_i18n::messages::command_configuration_removal_cancelled(
                nan_harness_i18n::locale()
            )
        );
        return Ok(());
    }
    for (harness, outcome) in manager.remove_all()? {
        print_removal(harness, outcome);
    }
    if pen_desktop::remove_persistent_configuration()? {
        println!(
            "{}",
            nan_harness_i18n::messages::command_nan_configuration_removed_from_pen_desktop(
                nan_harness_i18n::locale()
            )
        );
    }
    Ok(())
}

fn run_remove(
    manager: &ConfigurationManager,
    harness: Option<HarnessKind>,
) -> Result<(), ConfigurationError> {
    let harness = harness.ok_or(ConfigurationError::HarnessRequired)?;
    print_removal(harness, manager.remove(harness)?);
    Ok(())
}

async fn run_refresh_all(
    manager: &ConfigurationManager,
    interactive: bool,
) -> Result<(), ConfigurationError> {
    let configured = manager.configured_harnesses()?;
    let pen_configured = pen_desktop::persistent_configuration_exists()?;
    if configured.is_empty() && !pen_configured {
        println!("{}", nan_harness_i18n::messages::command_no_harness_configurations_are_managed_by_nan_harness(nan_harness_i18n::locale()));
        return Ok(());
    }
    let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
    for harness in configured {
        let change = manager.configure_with_media(harness, &config, &models, None, None)?;
        print_change(harness, &change, true);
    }
    if pen_configured {
        pen_desktop::refresh_persistent_with_config(&config, &models)?;
        println!(
            "{}", nan_harness_i18n::messages::command_nan_was_refreshed_for_pen_desktop_with_available_models(nan_harness_i18n::locale(), &(models.len())));
    }
    Ok(())
}

async fn configure_harness(
    manager: &ConfigurationManager,
    arguments: &ConfigArgs,
    interactive: bool,
) -> Result<(), ConfigurationError> {
    let harness = arguments
        .harness
        .and_then(ConfigTarget::stable)
        .ok_or(ConfigurationError::HarnessRequired)?;
    let already_configured = manager.is_configured(harness)?;
    if arguments.refresh && !already_configured {
        return Err(ConfigurationError::RefreshRequiresConfiguration(harness));
    }
    let requested_media = requested_media(arguments);
    if already_configured
        && !arguments.refresh
        && requested_search_policy(arguments).is_none()
        && requested_media.is_none()
    {
        print_status(manager, harness)?;
        println!(
            "{}",
            nan_harness_i18n::messages::command_refresh_it_with_nanh_config_refresh(
                nan_harness_i18n::locale(),
                &(harness)
            )
        );
        return Ok(());
    }
    if !already_configured
        && !arguments.yes
        && !confirm_configuration(
            manager,
            harness,
            requested_search_policy(arguments).unwrap_or_default(),
            requested_media,
            interactive,
        )?
    {
        println!(
            "{}",
            nan_harness_i18n::messages::command_configuration_cancelled(nan_harness_i18n::locale())
        );
        return Ok(());
    }
    let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
    let change = manager.configure_with_media(
        harness,
        &config,
        &models,
        requested_search_policy(arguments),
        requested_media,
    )?;
    print_change(harness, &change, arguments.refresh || already_configured);
    Ok(())
}

fn validate_arguments(arguments: &ConfigArgs) -> Result<(), ConfigurationError> {
    if arguments.harness.is_none()
        && !arguments.status
        && !arguments.refresh_all
        && !arguments.remove_all
    {
        return Err(ConfigurationError::HarnessRequired);
    }
    if arguments.yes
        && (arguments.harness.is_none() && !arguments.remove_all
            || arguments.status
            || arguments.refresh
            || arguments.remove
            || arguments.refresh_all)
    {
        return Err(ConfigurationError::UnusedYes);
    }
    if (arguments.search.no_search || arguments.search.force_search)
        && (arguments.status || arguments.remove || arguments.remove_all || arguments.refresh_all)
    {
        return Err(ConfigurationError::UnusedSearchPolicy);
    }
    if requested_media(arguments).is_some()
        && (arguments.status || arguments.remove || arguments.remove_all || arguments.refresh_all)
    {
        return Err(ConfigurationError::UnusedMediaPolicy);
    }
    Ok(())
}

fn requested_search_policy(arguments: &ConfigArgs) -> Option<WebSearchPolicy> {
    if arguments.search.no_search {
        Some(WebSearchPolicy::Disabled)
    } else if arguments.search.force_search {
        Some(WebSearchPolicy::Force)
    } else {
        None
    }
}

fn requested_media(arguments: &ConfigArgs) -> Option<MediaSelection> {
    crate::commands::media_policy::requested_media(&arguments.media)
}

fn confirm_configuration(
    manager: &ConfigurationManager,
    harness: HarnessKind,
    search_policy: WebSearchPolicy,
    media_request: Option<MediaSelection>,
    interactive: bool,
) -> Result<bool, ConfigurationError> {
    if !interactive {
        return Err(ConfigurationError::ConfirmationRequired);
    }
    eprintln!(
        "{}",
        nan_harness_i18n::messages::command_nan_harness_will_configure_nan_directly_in(
            nan_harness_i18n::locale(),
            &(harness)
        )
    );
    eprintln!(
        "{}", nan_harness_i18n::messages::command_this_copies_the_api_key_saved_by_nan_harness_into_the_harness_s_native_cred(nan_harness_i18n::locale()));
    eprintln!("{}", nan_harness_i18n::messages::command_nan_api_key_from_the_current_environment_will_not_be_copied(nan_harness_i18n::locale()));
    let search_managed = manager.resolve_managed_search(harness, search_policy, false)?;
    let working_directory = env::current_dir().map_err(ConfigurationError::CurrentDirectory)?;
    let media = crate::commands::media_policy::configuration_media(
        harness,
        media_request,
        MediaSelection::none(),
        &manager.paths.home_directory,
        &working_directory,
    );
    explain_search_confirmation(
        harness,
        ManagedSearchStatus {
            policy: search_policy,
            managed: search_managed,
        },
    );
    eprintln!(
        "{}",
        nan_harness_i18n::messages::command_files_nan_harness_will_manage(
            nan_harness_i18n::locale()
        )
    );
    for path in manager.paths_for_media(harness, search_managed, media)? {
        eprintln!("  - {}", path.display());
    }
    prompt_yes_no(&nan_harness_i18n::messages::prompt_continue(
        nan_harness_i18n::locale(),
    ))
}

fn confirm_remove_all(
    manager: &ConfigurationManager,
    yes: bool,
    interactive: bool,
) -> Result<bool, ConfigurationError> {
    if yes
        || manager.configured_harnesses()?.is_empty()
            && !pen_desktop::persistent_configuration_exists()?
    {
        return Ok(true);
    }
    if !interactive {
        return Err(ConfigurationError::ConfirmationRequired);
    }
    eprintln!("{}", nan_harness_i18n::messages::command_remove_every_harness_configuration_managed_by_nan_harness(nan_harness_i18n::locale()));
    prompt_yes_no(&nan_harness_i18n::messages::prompt_continue(
        nan_harness_i18n::locale(),
    ))
}

fn prompt_yes_no(prompt: &str) -> Result<bool, ConfigurationError> {
    let mut output = std::io::stderr().lock();
    write!(output, "{prompt}").map_err(ConfigurationError::Prompt)?;
    output.flush().map_err(ConfigurationError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(ConfigurationError::Prompt)?;
    Ok(nan_harness_i18n::yes_no(nan_harness_i18n::locale(), &response) == Some(true))
}

fn print_change(harness: HarnessKind, change: &ConfigurationChange, refreshed: bool) {
    let locale = nan_harness_i18n::locale();
    let count = change.model_count;
    let quantity = u64::try_from(count).unwrap_or(u64::MAX);
    let message = if !change.changed {
        nan_harness_i18n::messages::configuration_current_models(locale, quantity, &count, &harness)
    } else if refreshed {
        nan_harness_i18n::messages::configuration_refreshed_models(
            locale, quantity, &count, &harness,
        )
    } else {
        nan_harness_i18n::messages::configuration_configured_models(
            locale, quantity, &count, &harness,
        )
    };
    println!("{message}");
    println!(
        "{}",
        nan_harness_i18n::messages::command_web_search(
            nan_harness_i18n::locale(),
            &(search_status_summary(harness, change.search))
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::command_native_media(
            nan_harness_i18n::locale(),
            &media_status_summary(change.media)
        )
    );
    for path in &change.paths {
        println!(
            "{}",
            nan_harness_i18n::messages::command_managed(
                nan_harness_i18n::locale(),
                &(path.display())
            )
        );
    }
    println!(
        "{}",
        nan_harness_i18n::messages::command_run_directly_to_use_this_native_configuration(
            nan_harness_i18n::locale(),
            &(harness.binary_name())
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::command_refresh_later_with_nanh_config_refresh(
            nan_harness_i18n::locale(),
            &(harness)
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::command_remove_it_with_nanh_config_remove(
            nan_harness_i18n::locale(),
            &(harness)
        )
    );
}

fn print_removal(harness: HarnessKind, outcome: RemovalOutcome) {
    match outcome {
        RemovalOutcome::Removed => println!(
            "{}",
            nan_harness_i18n::messages::command_nan_configuration_removed_from(
                nan_harness_i18n::locale(),
                &(harness)
            )
        ),
        RemovalOutcome::NotConfigured => {
            println!("{}", nan_harness_i18n::messages::command_no_nan_configuration_managed_by_nan_harness_was_found_for(nan_harness_i18n::locale(), &(harness)));
        }
    }
}

fn print_status(
    manager: &ConfigurationManager,
    harness: HarnessKind,
) -> Result<(), ConfigurationError> {
    ensure_supported(harness)?;
    let Some(health) = manager.inspect(harness)? else {
        println!(
            "{}",
            nan_harness_i18n::messages::command_not_configured_by_nan_harness(
                nan_harness_i18n::locale(),
                &(harness)
            )
        );
        return Ok(());
    };
    if health.is_active() {
        let saved_fingerprint = credentials::saved_credential_fingerprint()?;
        if manager.credential_is_current(harness, saved_fingerprint.as_deref())? == Some(true) {
            println!("{}", nan_harness_i18n::messages::command_configured_unchanged_and_using_the_current_saved_key(nan_harness_i18n::locale(), &(harness)));
        } else {
            println!(
                "{}", nan_harness_i18n::messages::command_configured_and_unchanged_but_its_copied_key_needs_nanh_config_refresh(nan_harness_i18n::locale(), &(harness)));
        }
    } else {
        println!(
            "{}",
            nan_harness_i18n::messages::command_managed_configuration(
                nan_harness_i18n::locale(),
                &(harness),
                &(health.terminal_label(nan_harness_i18n::locale()))
            )
        );
        if let Some(code) = health.error_code() {
            println!(
                "{}",
                nan_harness_i18n::messages::command_error(nan_harness_i18n::locale(), &(code))
            );
        }
        if let Some(hint) = health.recovery_hint() {
            println!("  {hint}");
        }
    }
    match manager.search_status(harness)? {
        Some(search) => println!("{}", nan_harness_i18n::messages::command_web_search_details(nan_harness_i18n::locale(), &(search_status_summary(harness, search)))),
        None => println!(
            "{}", nan_harness_i18n::messages::command_web_search_policy_not_recorded_refresh_this_configuration_to_record_automat(nan_harness_i18n::locale())),
    }
    if let Some(media) = manager.media_status(harness)? {
        println!(
            "{}",
            nan_harness_i18n::messages::command_native_media(
                nan_harness_i18n::locale(),
                &media_status_summary(media)
            )
        );
    }
    Ok(())
}

fn print_bridge_only(harness: HarnessKind) {
    println!("{}", nan_harness_i18n::messages::command_uses_launch_scoped_routing_and_is_not_modified_by_nanh_config(nan_harness_i18n::locale(), &(harness)));
    println!(
        "{}",
        nan_harness_i18n::messages::command_launch_it_with_nanh(
            nan_harness_i18n::locale(),
            &(harness.binary_name())
        )
    );
    println!("{}", nan_harness_i18n::messages::command_use_no_search_or_force_search_on_that_launch_when_needed(nan_harness_i18n::locale()));
}

fn explain_search_confirmation(harness: HarnessKind, search: ManagedSearchStatus) {
    eprintln!("{}", search_confirmation_message(harness, search));
}

fn search_confirmation_message(harness: HarnessKind, search: ManagedSearchStatus) -> &'static str {
    match (harness, search.policy, search.managed) {
        (_, WebSearchPolicy::Disabled, _) => {
            nan_harness_i18n::messages::terminal_nan_web_search_will_not_be_added_existing_search_configuration_will_be_preserved_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Aider, WebSearchPolicy::Auto, false) => {
            nan_harness_i18n::messages::terminal_aider_does_not_support_the_nan_web_search_fallback_existing_search_configuration_will_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Auto, true) => {
            nan_harness_i18n::messages::terminal_a_runtime_aware_nan_web_search_fallback_will_be_installed_it_activates_only_when_no_l_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Force, true) => {
            nan_harness_i18n::messages::terminal_nan_web_search_will_replace_any_package_provided_web_search_tool_for_this_harness_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Omp, WebSearchPolicy::Auto, true) => {
            nan_harness_i18n::messages::terminal_an_authenticated_native_omp_search_provider_will_be_preferred_nan_web_search_will_be_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Omp, WebSearchPolicy::Force, true) => {
            nan_harness_i18n::messages::terminal_nan_web_search_will_replace_omp_s_native_web_search_provider_for_this_harness_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Auto, true) => {
            nan_harness_i18n::messages::terminal_no_other_web_search_provider_was_detected_so_the_nan_fallback_will_be_added_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Auto, false) => {
            nan_harness_i18n::messages::terminal_an_existing_web_search_configuration_was_detected_so_nan_harness_will_preserve_it_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Force, true) => {
            nan_harness_i18n::messages::terminal_nan_web_search_will_be_added_even_if_another_provider_is_configured_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Force, false) => {
            nan_harness_i18n::messages::terminal_nan_web_search_is_already_configured_so_nan_harness_will_leave_that_entry_untouched_text(nan_harness_i18n::locale())
        }
    }
}

#[cfg(test)]
#[path = "command/search_confirmation_tests.rs"]
mod search_confirmation_tests;

fn search_status_summary(harness: HarnessKind, search: ManagedSearchStatus) -> &'static str {
    match (harness, search.policy, search.managed) {
        (_, WebSearchPolicy::Disabled, _) => {
            nan_harness_i18n::messages::terminal_nan_fallback_disabled_existing_search_configuration_preserved_text(nan_harness_i18n::locale())
        }
        (HarnessKind::Aider, WebSearchPolicy::Auto, false) => nan_harness_i18n::messages::terminal_nan_fallback_unavailable_for_aider_text(nan_harness_i18n::locale()),
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Auto, true) => {
            nan_harness_i18n::messages::terminal_runtime_aware_automatic_nan_fallback_installed_text(nan_harness_i18n::locale())
        }
        (
            HarnessKind::Pi | HarnessKind::Omp | HarnessKind::PrimeAgent,
            WebSearchPolicy::Force,
            true,
        ) => nan_harness_i18n::messages::terminal_forced_nan_search_override_installed_text(nan_harness_i18n::locale()),
        (HarnessKind::Omp, WebSearchPolicy::Auto, true) => {
            nan_harness_i18n::messages::terminal_authenticated_native_first_nan_fallback_installed_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Auto, true) => nan_harness_i18n::messages::terminal_automatic_nan_fallback_active_text(nan_harness_i18n::locale()),
        (_, WebSearchPolicy::Auto, false) => {
            nan_harness_i18n::messages::terminal_automatic_policy_existing_search_configuration_preserved_text(nan_harness_i18n::locale())
        }
        (_, WebSearchPolicy::Force, true) => nan_harness_i18n::messages::terminal_forced_nan_search_active_text(nan_harness_i18n::locale()),
        (_, WebSearchPolicy::Force, false) => {
            nan_harness_i18n::messages::terminal_force_policy_satisfied_by_an_existing_nan_search_entry_text(nan_harness_i18n::locale())
        }
    }
}

fn media_status_summary(media: MediaSelection) -> String {
    format!(
        "STT={}, TTS={}, image={}",
        if media.stt {
            "NaN Whisper"
        } else {
            "preserved"
        },
        if media.tts { "NaN Kokoro" } else { "preserved" },
        if media.image {
            "NaN Flux 2 Klein"
        } else {
            "preserved"
        },
    )
}

fn print_all_statuses(manager: &ConfigurationManager) -> Result<(), ConfigurationError> {
    for harness in SUPPORTED_HARNESSES {
        print_status(manager, harness)?;
    }
    println!(
        "{}",
        nan_harness_i18n::messages::command_claude_code_launch_only_use_nanh_claude(
            nan_harness_i18n::locale()
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::command_codex_launch_only_use_nanh_codex(
            nan_harness_i18n::locale()
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::command_fx_launch_only_use_nanh_fx(nan_harness_i18n::locale())
    );
    print_pen_status()?;
    Ok(())
}

async fn run_pen(arguments: &ConfigArgs, interactive: bool) -> Result<(), ConfigurationError> {
    if arguments.search.no_search || arguments.search.force_search {
        return Err(ConfigurationError::UnusedSearchPolicy);
    }
    if requested_media(arguments).is_some() {
        return Err(ConfigurationError::UnusedMediaPolicy);
    }
    if arguments.status {
        return print_pen_status();
    }
    if arguments.remove {
        if pen_desktop::remove_persistent_configuration()? {
            println!(
                "{}",
                nan_harness_i18n::messages::command_nan_configuration_removed_from_pen_desktop(
                    nan_harness_i18n::locale()
                )
            );
        } else {
            println!(
                "{}",
                nan_harness_i18n::messages::command_pen_desktop_not_configured_by_nan_harness(
                    nan_harness_i18n::locale()
                )
            );
        }
        return Ok(());
    }
    let configured = pen_desktop::persistent_configuration_exists()?;
    if arguments.refresh && !configured {
        return Err(ConfigurationError::PenNotConfigured);
    }
    if configured && !arguments.refresh {
        print_pen_status()?;
        println!(
            "{}",
            nan_harness_i18n::messages::command_refresh_it_with_nanh_config_pen_refresh(
                nan_harness_i18n::locale()
            )
        );
        return Ok(());
    }
    match pen_desktop::configure_persistent(arguments.refresh, arguments.yes, interactive).await {
        Ok(count) => {
            let locale = nan_harness_i18n::locale();
            let quantity = u64::try_from(count).unwrap_or(u64::MAX);
            let message = if arguments.refresh {
                nan_harness_i18n::messages::configuration_refreshed_models(
                    locale,
                    quantity,
                    &count,
                    "Pen Desktop",
                )
            } else {
                nan_harness_i18n::messages::configuration_configured_models(
                    locale,
                    quantity,
                    &count,
                    "Pen Desktop",
                )
            };
            println!("{message}");
            println!("{}", nan_harness_i18n::messages::command_restart_pen_completely_to_reload_its_model_catalog(nan_harness_i18n::locale()));
            println!(
                "{}",
                nan_harness_i18n::messages::command_remove_it_with_nanh_config_pen_remove(
                    nan_harness_i18n::locale()
                )
            );
            Ok(())
        }
        Err(PenDesktopError::ConfigurationCancelled) => {
            println!(
                "{}",
                nan_harness_i18n::messages::command_configuration_cancelled(
                    nan_harness_i18n::locale()
                )
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn print_pen_status() -> Result<(), ConfigurationError> {
    let Some(model_count) = pen_desktop::persistent_model_count()? else {
        println!(
            "{}",
            nan_harness_i18n::messages::command_pen_desktop_not_configured_by_nan_harness(
                nan_harness_i18n::locale()
            )
        );
        return Ok(());
    };
    let health =
        pen_desktop::inspect_persistent_configuration()?.unwrap_or(ConfigurationHealth::Missing);
    if health.is_active() {
        let saved_fingerprint = credentials::saved_credential_fingerprint()?;
        if pen_desktop::persistent_credential_is_current(saved_fingerprint.as_deref())?
            == Some(true)
        {
            println!(
                "{}", nan_harness_i18n::messages::command_pen_desktop_configured_unchanged_and_using_the_current_saved_key_models(nan_harness_i18n::locale(), &(model_count)));
        } else {
            println!(
                "{}", nan_harness_i18n::messages::command_pen_desktop_configured_and_unchanged_but_its_copied_key_needs_nanh_config_p(nan_harness_i18n::locale(), &(model_count)));
        }
    } else {
        println!(
            "{}",
            nan_harness_i18n::messages::command_pen_desktop_managed_configuration(
                nan_harness_i18n::locale(),
                &(health.terminal_label(nan_harness_i18n::locale()))
            )
        );
        if let Some(code) = health.error_code() {
            println!(
                "{}",
                nan_harness_i18n::messages::command_error(nan_harness_i18n::locale(), &(code))
            );
        }
        if let Some(hint) = health.recovery_hint() {
            println!("  {hint}");
        }
    }
    Ok(())
}
