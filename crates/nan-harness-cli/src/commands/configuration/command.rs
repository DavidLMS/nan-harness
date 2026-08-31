use super::*;
use crate::app::ConfigArgs;
use crate::commands::credentials;
use nan_harness_core::WebSearchPolicy;
use std::io::{BufRead as _, Write as _};

pub(crate) async fn run(
    arguments: &ConfigArgs,
    interactive: bool,
) -> Result<(), ConfigurationError> {
    validate_arguments(arguments)?;
    let manager = ConfigurationManager::from_environment()?;

    if let Some(harness) = arguments.harness
        && !SUPPORTED_HARNESSES.contains(&harness)
    {
        print_bridge_only(harness);
        return Ok(());
    }

    if arguments.status {
        if let Some(harness) = arguments.harness {
            print_status(&manager, harness)?;
        } else {
            print_all_statuses(&manager)?;
        }
        return Ok(());
    }
    if arguments.remove_all {
        if !confirm_remove_all(&manager, arguments.yes, interactive)? {
            println!("Configuration removal cancelled.");
            return Ok(());
        }
        let outcomes = manager.remove_all()?;
        for (harness, outcome) in outcomes {
            print_removal(harness, outcome);
        }
        return Ok(());
    }
    if arguments.remove {
        let Some(harness) = arguments.harness else {
            return Err(ConfigurationError::HarnessRequired);
        };
        print_removal(harness, manager.remove(harness)?);
        return Ok(());
    }

    if arguments.refresh_all {
        let configured = manager.configured_harnesses()?;
        if configured.is_empty() {
            println!("No harness configurations are managed by nan-harness.");
            return Ok(());
        }
        let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
        for harness in configured {
            let change = manager.configure(harness, &config, &models, None)?;
            print_change(harness, &change, true);
        }
        return Ok(());
    }

    let harness = arguments
        .harness
        .ok_or(ConfigurationError::HarnessRequired)?;
    let already_configured = manager.is_configured(harness)?;
    if arguments.refresh && !already_configured {
        return Err(ConfigurationError::RefreshRequiresConfiguration(harness));
    }
    if already_configured && !arguments.refresh && requested_search_policy(arguments).is_none() {
        print_status(&manager, harness)?;
        println!("Refresh it with `nan config {harness} --refresh`.");
        return Ok(());
    }
    if !already_configured
        && !arguments.yes
        && !confirm_configuration(
            &manager,
            harness,
            requested_search_policy(arguments).unwrap_or_default(),
            interactive,
        )?
    {
        println!("Configuration cancelled.");
        return Ok(());
    }
    let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
    let change = manager.configure(
        harness,
        &config,
        &models,
        requested_search_policy(arguments),
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

fn confirm_configuration(
    manager: &ConfigurationManager,
    harness: HarnessKind,
    search_policy: WebSearchPolicy,
    interactive: bool,
) -> Result<bool, ConfigurationError> {
    if !interactive {
        return Err(ConfigurationError::ConfirmationRequired);
    }
    eprintln!("nan-harness will configure NaN directly in {harness}.");
    eprintln!(
        "This copies the API key saved by nan-harness into the harness's native credential storage."
    );
    eprintln!("NAN_API_KEY from the current environment will not be copied.");
    let search_managed = manager.resolve_managed_search(harness, search_policy, false)?;
    explain_search_confirmation(
        harness,
        ManagedSearchStatus {
            policy: search_policy,
            managed: search_managed,
        },
    );
    eprintln!("Files nan-harness will manage:");
    for path in manager.paths_for_search(harness, search_managed)? {
        eprintln!("  - {}", path.display());
    }
    prompt_yes_no("Continue? [y/N] ")
}

fn confirm_remove_all(
    manager: &ConfigurationManager,
    yes: bool,
    interactive: bool,
) -> Result<bool, ConfigurationError> {
    if yes || manager.configured_harnesses()?.is_empty() {
        return Ok(true);
    }
    if !interactive {
        return Err(ConfigurationError::ConfirmationRequired);
    }
    eprintln!("Remove every harness configuration managed by nan-harness?");
    prompt_yes_no("Continue? [y/N] ")
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
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn print_change(harness: HarnessKind, change: &ConfigurationChange, refreshed: bool) {
    let action = if !change.changed {
        "is already up to date"
    } else if refreshed {
        "was refreshed"
    } else {
        "was configured"
    };
    println!(
        "NaN {action} for {harness} with {} available models.",
        change.model_count
    );
    println!(
        "Web search: {}.",
        search_status_summary(harness, change.search)
    );
    for path in &change.paths {
        println!("Managed: {}", path.display());
    }
    println!(
        "Run `{}` directly to use this native configuration.",
        harness.binary_name()
    );
    println!("Refresh later with `nan config {harness} --refresh`.");
    println!("Remove it with `nan config {harness} --remove`.");
}

fn print_removal(harness: HarnessKind, outcome: RemovalOutcome) {
    match outcome {
        RemovalOutcome::Removed => println!("NaN configuration removed from {harness}."),
        RemovalOutcome::NotConfigured => {
            println!("No NaN configuration managed by nan-harness was found for {harness}.");
        }
    }
}

fn print_status(
    manager: &ConfigurationManager,
    harness: HarnessKind,
) -> Result<(), ConfigurationError> {
    ensure_supported(harness)?;
    if !manager.is_configured(harness)? {
        println!("{harness}: not configured by nan-harness");
    } else if manager.is_active(harness)? {
        let saved_fingerprint = credentials::saved_credential_fingerprint()?;
        if manager.credential_is_current(harness, saved_fingerprint.as_deref())? == Some(true) {
            println!("{harness}: configured, unchanged, and using the current saved key");
        } else {
            println!(
                "{harness}: configured and unchanged, but its copied key needs `nan config {harness} --refresh`"
            );
        }
    } else {
        println!("{harness}: managed configuration changed or is incomplete");
    }
    if manager.is_configured(harness)? {
        match manager.search_status(harness)? {
            Some(search) => println!("  Web search: {}.", search_status_summary(harness, search)),
            None => println!(
                "  Web search: policy not recorded; refresh this configuration to record automatic selection."
            ),
        }
    }
    Ok(())
}

fn print_bridge_only(harness: HarnessKind) {
    println!("{harness} uses launch-scoped routing and is not modified by `nan config`.");
    println!("Launch it with `nan {}`.", harness.binary_name());
    println!("Use --no-search or --force-search on that launch when needed.");
}

fn explain_search_confirmation(harness: HarnessKind, search: ManagedSearchStatus) {
    let message = match (harness, search.policy, search.managed) {
        (_, WebSearchPolicy::Disabled, _) => {
            "NaN web search will not be added; existing search configuration will be preserved."
        }
        (HarnessKind::Aider, WebSearchPolicy::Auto, false) => {
            "Aider does not support the NaN web search fallback; existing search configuration will be preserved."
        }
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Auto, true) => {
            "A runtime-aware NaN web search fallback will be installed; it activates only when no loaded extension provides web_search."
        }
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Force, true) => {
            "NaN web search will replace any package-provided web_search tool for this harness."
        }
        (_, WebSearchPolicy::Auto, true) => {
            "No other web search provider was detected, so the NaN fallback will be added."
        }
        (_, WebSearchPolicy::Auto, false) => {
            "An existing web search configuration was detected, so nan-harness will preserve it."
        }
        (_, WebSearchPolicy::Force, true) => {
            "NaN web search will be added even if another provider is configured."
        }
        (_, WebSearchPolicy::Force, false) => {
            "NaN web search is already configured, so nan-harness will leave that entry untouched."
        }
    };
    eprintln!("{message}");
}

fn search_status_summary(harness: HarnessKind, search: ManagedSearchStatus) -> &'static str {
    match (harness, search.policy, search.managed) {
        (_, WebSearchPolicy::Disabled, _) => {
            "NaN fallback disabled; existing search configuration preserved"
        }
        (HarnessKind::Aider, WebSearchPolicy::Auto, false) => "NaN fallback unavailable for Aider",
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Auto, true) => {
            "runtime-aware automatic NaN fallback installed"
        }
        (HarnessKind::Pi | HarnessKind::PrimeAgent, WebSearchPolicy::Force, true) => {
            "forced NaN search override installed"
        }
        (_, WebSearchPolicy::Auto, true) => "automatic NaN fallback active",
        (_, WebSearchPolicy::Auto, false) => {
            "automatic policy; existing search configuration preserved"
        }
        (_, WebSearchPolicy::Force, true) => "forced NaN search active",
        (_, WebSearchPolicy::Force, false) => {
            "force policy satisfied by an existing NaN search entry"
        }
    }
}

fn print_all_statuses(manager: &ConfigurationManager) -> Result<(), ConfigurationError> {
    for harness in SUPPORTED_HARNESSES {
        print_status(manager, harness)?;
    }
    println!("claude-code: launch-only; use `nan claude`");
    println!("codex: launch-only; use `nan codex`");
    println!("fx: launch-only; use `nan fx`");
    Ok(())
}
