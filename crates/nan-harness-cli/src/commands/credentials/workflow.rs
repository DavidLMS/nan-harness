use super::verification::{is_rejected, verify_cached, verify_models};
use super::{CredentialError, CredentialManager, CredentialSource, credential_fingerprint, verify};
use crate::app::AuthCommand;
use crate::commands::configuration::ConfigurationManager;
use crate::commands::pen_desktop;
use nan_harness_core::{CodingModelProfile, SecretValue};
use nan_harness_runtime::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
use std::env;
use std::io::{BufRead as _, Write as _};

const NAN_GET_API_KEY_URL: &str = "https://nan.builders/";

#[derive(Debug)]
pub(crate) struct ResolvedLaunchConfig {
    pub(crate) config: ResolvedConfig,
    pub(crate) model_catalog: Option<Vec<CodingModelProfile>>,
}

pub(crate) async fn run(command: &AuthCommand, interactive: bool) -> Result<(), CredentialError> {
    let manager = CredentialManager::from_environment()?;
    match command {
        AuthCommand::Login => {
            if !interactive {
                return Err(CredentialError::InteractiveLoginRequired);
            }
            let (config, _, models) =
                prompt_and_store(&ProcessEnvironment, &manager, None, false, prompt_api_key)
                    .await?;
            offer_configuration_refresh(&config, &models, interactive)?;
        }
        AuthCommand::Status => print_status(&manager).await?,
        AuthCommand::Logout(arguments) => {
            if !prepare_logout(*arguments, interactive)? {
                println!("Logout cancelled.");
                return Ok(());
            }
            if manager.remove_saved()? {
                println!("Saved NaN API key removed.");
            } else {
                println!("No saved NaN API key is configured.");
            }
            if env::var_os("NAN_API_KEY").is_some_and(|value| !value.is_empty()) {
                println!("NAN_API_KEY remains set and takes precedence.");
            }
        }
    }
    Ok(())
}

pub(crate) async fn resolve_saved_or_onboard(
    provider_base_url: Option<String>,
    interactive: bool,
) -> Result<(ResolvedConfig, Vec<CodingModelProfile>), CredentialError> {
    let manager = CredentialManager::from_environment()?;
    if let Some((api_key, source)) = manager.load()? {
        let config = ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: provider_base_url.clone(),
                nan_api_key: Some(api_key),
            },
        )?;
        match verify_models(&config).await {
            Ok(models) => return Ok((config, models)),
            Err(error) if is_rejected(&error) && interactive => {
                eprintln!("The NaN API key from {source} was rejected by the provider.");
                if !prompt_yes_no("Enter and save a replacement NaN API key now? [Y/n] ", true)? {
                    return Err(error);
                }
                let (replacement, _, models) = prompt_and_store(
                    &ProcessEnvironment,
                    &manager,
                    provider_base_url,
                    false,
                    prompt_api_key,
                )
                .await?;
                eprintln!(
                    "Other managed harness configurations still contain the previous key; update them with `nanh config --refresh-all`."
                );
                return Ok((replacement, models));
            }
            Err(error) => return Err(error),
        }
    }
    if !interactive {
        return Err(CredentialError::MissingSavedCredential);
    }
    prompt_and_store(
        &ProcessEnvironment,
        &manager,
        provider_base_url,
        true,
        prompt_api_key,
    )
    .await
    .map(|(config, _, models)| (config, models))
}

pub(crate) fn resolve_existing_config(
    provider_base_url: Option<String>,
) -> Result<Option<ResolvedConfig>, CredentialError> {
    let manager = CredentialManager::from_environment()?;
    existing_config(&ProcessEnvironment, &manager, provider_base_url)
        .map(|resolved| resolved.map(|(config, _)| config))
}

pub(crate) fn saved_credential_fingerprint() -> Result<Option<String>, CredentialError> {
    let manager = CredentialManager::from_environment()?;
    saved_config(&manager, None)?
        .map(|(config, _)| credential_fingerprint(&config))
        .transpose()
}

pub(crate) async fn resolve_or_onboard(
    provider_base_url: Option<String>,
    interactive: bool,
) -> Result<ResolvedLaunchConfig, CredentialError> {
    let manager = CredentialManager::from_environment()?;
    if let Some((config, source)) =
        existing_config(&ProcessEnvironment, &manager, provider_base_url.clone())?
    {
        match verify_cached(&config).await {
            Ok(model_catalog) => {
                return Ok(ResolvedLaunchConfig {
                    config,
                    model_catalog,
                });
            }
            Err(error) if is_rejected(&error) && interactive => {
                return recover_rejected_credential(&manager, provider_base_url, source, error)
                    .await;
            }
            Err(error) => return Err(error),
        }
    }
    if !interactive {
        return Err(CredentialError::MissingCredential);
    }
    prompt_and_store(
        &ProcessEnvironment,
        &manager,
        provider_base_url,
        true,
        prompt_api_key,
    )
    .await
    .map(|(config, _, models)| ResolvedLaunchConfig {
        config,
        model_catalog: Some(models),
    })
}

#[cfg(test)]
pub(super) async fn resolve_or_onboard_with(
    environment: &impl EnvironmentSource,
    manager: &CredentialManager,
    provider_base_url: Option<String>,
    interactive: bool,
    prompt: impl FnOnce() -> Result<SecretValue, CredentialError>,
) -> Result<ResolvedConfig, CredentialError> {
    if let Some((config, _)) = existing_config(environment, manager, provider_base_url.clone())? {
        return Ok(config);
    }
    if !interactive {
        return Err(CredentialError::MissingCredential);
    }
    prompt_and_store(environment, manager, provider_base_url, true, prompt)
        .await
        .map(|(config, _, _)| config)
}

async fn prompt_and_store(
    environment: &impl EnvironmentSource,
    manager: &CredentialManager,
    provider_base_url: Option<String>,
    announce_missing: bool,
    prompt: impl FnOnce() -> Result<SecretValue, CredentialError>,
) -> Result<(ResolvedConfig, CredentialSource, Vec<CodingModelProfile>), CredentialError> {
    if announce_missing {
        eprintln!("NAN_API_KEY is not configured.");
        eprintln!("{}", render_missing_credential_hint());
    }
    eprintln!("Enter your NaN API key to verify and save it for future commands.");
    let api_key = prompt()?;
    let config = ConfigResolver::resolve(
        environment,
        ConfigOverrides {
            provider_base_url,
            nan_api_key: Some(api_key),
        },
    )?;
    let models = verify_models(&config).await?;
    let source = config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| manager.save(value))
        .map_err(CredentialError::Secret)??;
    match source {
        CredentialSource::SystemKeyring => {
            eprintln!("NaN API key verified and saved in the system credential store.");
        }
        CredentialSource::PrivateFile => {
            eprintln!(
                "warning: the system credential store is unavailable; the verified API key was saved in a private nan-harness credential file"
            );
        }
        CredentialSource::Environment => unreachable!("prompted credentials are persisted"),
    }
    if let Some(message) = render_first_harness_hint(announce_missing) {
        eprintln!("{message}");
    }
    Ok((config, source, models))
}

pub(super) fn render_missing_credential_hint() -> String {
    format!("Get one at {NAN_GET_API_KEY_URL}")
}

pub(super) fn render_first_harness_hint(announce_missing: bool) -> Option<&'static str> {
    announce_missing.then_some("Start your first harness with: nanh pi")
}

fn existing_config(
    environment: &impl EnvironmentSource,
    manager: &CredentialManager,
    provider_base_url: Option<String>,
) -> Result<Option<(ResolvedConfig, CredentialSource)>, CredentialError> {
    match ConfigResolver::resolve(
        environment,
        ConfigOverrides {
            provider_base_url: provider_base_url.clone(),
            nan_api_key: None,
        },
    ) {
        Ok(config) => return Ok(Some((config, CredentialSource::Environment))),
        Err(ConfigError::MissingApiKey) => {}
        Err(error) => return Err(CredentialError::Config(error)),
    }
    let Some((api_key, source)) = manager.load()? else {
        return Ok(None);
    };
    ConfigResolver::resolve(
        environment,
        ConfigOverrides {
            provider_base_url,
            nan_api_key: Some(api_key),
        },
    )
    .map(|config| Some((config, source)))
    .map_err(CredentialError::Config)
}

async fn recover_rejected_credential(
    manager: &CredentialManager,
    provider_base_url: Option<String>,
    source: CredentialSource,
    original_error: CredentialError,
) -> Result<ResolvedLaunchConfig, CredentialError> {
    eprintln!("The NaN API key from {source} was rejected by the provider.");
    if source == CredentialSource::Environment
        && let Some((saved, saved_source)) = saved_config(manager, provider_base_url.clone())?
        && prompt_yes_no(
            "Try the API key saved by nan-harness for this launch? [Y/n] ",
            true,
        )?
    {
        match verify_models(&saved).await {
            Ok(models) => {
                eprintln!("Using the key from {saved_source} for this launch.");
                eprintln!(
                    "NAN_API_KEY will take precedence again on the next launch until it is updated or unset."
                );
                return Ok(ResolvedLaunchConfig {
                    config: saved,
                    model_catalog: Some(models),
                });
            }
            Err(error) if is_rejected(&error) => {
                eprintln!("The saved NaN API key was also rejected.");
            }
            Err(error) => return Err(error),
        }
    }
    if !prompt_yes_no("Enter and save a replacement NaN API key now? [Y/n] ", true)? {
        return Err(original_error);
    }
    let (config, _, models) = prompt_and_store(
        &ProcessEnvironment,
        manager,
        provider_base_url,
        false,
        prompt_api_key,
    )
    .await?;
    if source == CredentialSource::Environment {
        eprintln!(
            "The replacement was saved, but NAN_API_KEY still wins on future launches until it is updated or unset."
        );
    } else {
        let configuration_manager = ConfigurationManager::from_environment()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        let has_native = !configuration_manager
            .configured_harnesses()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?
            .is_empty();
        let has_pen = pen_desktop::persistent_configuration_exists()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        if has_native || has_pen {
            eprintln!(
                "Managed harness configurations still contain the previous key; update them with `nanh config --refresh-all`."
            );
        }
    }
    Ok(ResolvedLaunchConfig {
        config,
        model_catalog: Some(models),
    })
}

async fn print_status(manager: &CredentialManager) -> Result<(), CredentialError> {
    let environment = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: None,
            nan_api_key: None,
        },
    );
    match environment {
        Ok(config) => print_health("Effective launch key", "NAN_API_KEY", verify(&config).await),
        Err(ConfigError::MissingApiKey) => {
            println!("Effective launch key: not set in NAN_API_KEY.");
        }
        Err(error) => return Err(CredentialError::Config(error)),
    }
    let saved = saved_config(manager, None)?;
    let saved_fingerprint = saved
        .as_ref()
        .map(|(config, _)| credential_fingerprint(config))
        .transpose()?;
    match saved {
        Some((config, source)) => {
            print_health(
                "Saved configuration key",
                &source.to_string(),
                verify(&config).await,
            );
        }
        None => println!("Saved configuration key: not configured."),
    }
    let configuration_manager = ConfigurationManager::from_environment()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let configured = configuration_manager
        .configured_harnesses()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let mut changed = 0;
    for harness in &configured {
        let active = configuration_manager
            .is_active(*harness)
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        let credential_current = configuration_manager
            .credential_is_current(*harness, saved_fingerprint.as_deref())
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?
            == Some(true);
        if !active || !credential_current {
            changed += 1;
        }
    }
    let pen_configured = pen_desktop::persistent_configuration_exists()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    if pen_configured {
        let active = pen_desktop::persistent_configuration_active()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        let credential_current =
            pen_desktop::persistent_credential_is_current(saved_fingerprint.as_deref())
                .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?
                == Some(true);
        if !active || !credential_current {
            changed += 1;
        }
    }
    println!(
        "Managed harness configurations: {} total, {} needing attention.",
        configured.len() + usize::from(pen_configured),
        changed
    );
    Ok(())
}

fn print_health(label: &str, source: &str, result: Result<(), CredentialError>) {
    match result {
        Ok(()) => println!("{label}: valid through {source}."),
        Err(error) if is_rejected(&error) => {
            println!("{label}: rejected by the provider through {source}.");
        }
        Err(error) => println!("{label}: could not be verified through {source}: {error}"),
    }
}

fn offer_configuration_refresh(
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
    interactive: bool,
) -> Result<(), CredentialError> {
    let manager = ConfigurationManager::from_environment()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let configured = manager
        .configured_harnesses()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let pen_configured = pen_desktop::persistent_configuration_exists()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    if configured.is_empty() && !pen_configured {
        return Ok(());
    }
    if !interactive
        || !prompt_yes_no(
            "Update all harness configurations managed by nan-harness with this key? [Y/n] ",
            true,
        )?
    {
        println!("Run `nanh config --refresh-all` when you want to update them.");
        return Ok(());
    }
    for harness in configured {
        manager
            .configure(harness, config, models, None)
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        println!("Updated the managed {harness} configuration.");
    }
    if pen_configured
        && pen_desktop::refresh_persistent_with_config(config, models)
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?
    {
        println!("Updated the managed Pen Desktop configuration.");
    }
    Ok(())
}

fn prepare_logout(
    arguments: crate::app::AuthLogoutArgs,
    interactive: bool,
) -> Result<bool, CredentialError> {
    let configuration_manager = ConfigurationManager::from_environment()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let configured = configuration_manager
        .configured_harnesses()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    let pen_configured = pen_desktop::persistent_configuration_exists()
        .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
    if configured.is_empty() && !pen_configured {
        if !interactive && !arguments.yes {
            return Err(CredentialError::LogoutConfirmationRequired);
        }
        return Ok(true);
    }
    let remove_configs = if interactive && !arguments.yes {
        eprintln!(
            "The saved key has been copied into {} managed harness configurations.",
            configured.len() + usize::from(pen_configured)
        );
        eprintln!("  1. Remove the saved key and all managed harness configurations (recommended)");
        eprintln!("  2. Remove only the saved key and keep harness configurations");
        eprintln!("  3. Cancel");
        let Some(remove_configs) = prompt_logout_choice()? else {
            return Ok(false);
        };
        remove_configs
    } else {
        if !arguments.yes || arguments.remove_configs == arguments.keep_configs {
            return Err(CredentialError::LogoutModeRequired);
        }
        arguments.remove_configs
    };
    if remove_configs {
        configuration_manager
            .remove_all()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        if pen_configured {
            pen_desktop::remove_persistent_configuration()
                .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        }
        println!("All managed harness configurations were removed.");
    }
    Ok(true)
}

fn prompt_logout_choice() -> Result<Option<bool>, CredentialError> {
    let mut output = std::io::stderr().lock();
    write!(output, "Choose [1]: ").map_err(CredentialError::Prompt)?;
    output.flush().map_err(CredentialError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(CredentialError::Prompt)?;
    match response.trim() {
        "" | "1" => Ok(Some(true)),
        "2" => Ok(Some(false)),
        "3" => Ok(None),
        _ => Err(CredentialError::InvalidLogoutChoice),
    }
}

fn saved_config(
    manager: &CredentialManager,
    provider_base_url: Option<String>,
) -> Result<Option<(ResolvedConfig, CredentialSource)>, CredentialError> {
    let Some((api_key, source)) = manager.load()? else {
        return Ok(None);
    };
    ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url,
            nan_api_key: Some(api_key),
        },
    )
    .map(|config| Some((config, source)))
    .map_err(CredentialError::Config)
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool, CredentialError> {
    let mut output = std::io::stderr().lock();
    write!(output, "{prompt}").map_err(CredentialError::Prompt)?;
    output.flush().map_err(CredentialError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(CredentialError::Prompt)?;
    match response.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Ok(default),
    }
}

fn prompt_api_key() -> Result<SecretValue, CredentialError> {
    let api_key = rpassword::prompt_password("NaN API key (input hidden): ")
        .map_err(CredentialError::Prompt)?;
    SecretValue::new(api_key).map_err(CredentialError::Secret)
}
