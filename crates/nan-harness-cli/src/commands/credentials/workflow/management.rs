use super::super::verification::is_rejected;
use super::super::{CredentialError, CredentialManager, credential_fingerprint, verify};
use super::prompt::{prompt_logout_choice, prompt_yes_no};
use super::recovery::saved_config;
use crate::app::AuthLogoutArgs;
use crate::commands::configuration::ConfigurationManager;
use crate::commands::pen_desktop;
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::{
    ConfigError, ConfigOverrides, ConfigResolver, ProcessEnvironment, ResolvedConfig,
};

pub(super) async fn print_status(manager: &CredentialManager) -> Result<(), CredentialError> {
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

pub(super) fn offer_configuration_refresh(
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

pub(super) fn prepare_logout(
    arguments: AuthLogoutArgs,
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
        explicit_logout_mode(arguments)?
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

fn explicit_logout_mode(arguments: AuthLogoutArgs) -> Result<bool, CredentialError> {
    if !arguments.yes || arguments.remove_configs == arguments.keep_configs {
        return Err(CredentialError::LogoutModeRequired);
    }
    Ok(arguments.remove_configs)
}

#[cfg(test)]
mod tests {
    use super::explicit_logout_mode;
    use crate::app::AuthLogoutArgs;
    use crate::commands::credentials::CredentialError;

    fn arguments(remove_configs: bool, keep_configs: bool, yes: bool) -> AuthLogoutArgs {
        AuthLogoutArgs {
            remove_configs,
            keep_configs,
            yes,
        }
    }

    #[test]
    fn explicit_logout_requires_confirmation_and_exactly_one_mode() {
        for arguments in [
            arguments(false, false, false),
            arguments(true, false, false),
            arguments(false, true, false),
            arguments(false, false, true),
            arguments(true, true, true),
        ] {
            assert!(matches!(
                explicit_logout_mode(arguments),
                Err(CredentialError::LogoutModeRequired)
            ));
        }

        assert!(
            explicit_logout_mode(arguments(true, false, true))
                .expect("remove mode should be accepted")
        );
        assert!(
            !explicit_logout_mode(arguments(false, true, true))
                .expect("keep mode should be accepted")
        );
    }
}
