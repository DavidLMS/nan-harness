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
        Ok(config) => print_health(
            nan_harness_i18n::messages::terminal_effective_launch_key_text(
                nan_harness_i18n::locale(),
            ),
            "NAN_API_KEY",
            verify(&config).await,
        ),
        Err(ConfigError::MissingApiKey) => {
            println!(
                "{}",
                nan_harness_i18n::messages::management_effective_launch_key_not_set_in_nan_api_key(
                    nan_harness_i18n::locale()
                )
            );
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
                nan_harness_i18n::messages::terminal_saved_configuration_key_text(
                    nan_harness_i18n::locale(),
                ),
                source.terminal_label(),
                verify(&config).await,
            );
        }
        None => println!(
            "{}",
            nan_harness_i18n::messages::management_saved_configuration_key_not_configured(
                nan_harness_i18n::locale()
            )
        ),
    }
    let configuration_manager = ConfigurationManager::from_environment().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    let configured = configuration_manager
        .configured_harnesses()
        .map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?;
    let mut changed = 0;
    for harness in &configured {
        let active = configuration_manager.is_active(*harness).map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?;
        let credential_current = configuration_manager
            .credential_is_current(*harness, saved_fingerprint.as_deref())
            .map_err(|error| {
                CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
            })?
            == Some(true);
        if !active || !credential_current {
            changed += 1;
        }
    }
    let pen_configured = pen_desktop::persistent_configuration_exists().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    if pen_configured {
        let active = pen_desktop::persistent_configuration_active().map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?;
        let credential_current = pen_desktop::persistent_credential_is_current(
            saved_fingerprint.as_deref(),
        )
        .map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })? == Some(true);
        if !active || !credential_current {
            changed += 1;
        }
    }
    println!(
        "{}", nan_harness_i18n::messages::management_managed_harness_configurations_total_needing_attention(nan_harness_i18n::locale(), &(configured.len() + usize::from(pen_configured)), &(changed)));
    Ok(())
}

fn print_health(label: &str, source: &str, result: Result<(), CredentialError>) {
    match result {
        Ok(()) => println!(
            "{}",
            nan_harness_i18n::messages::management_valid_through(
                nan_harness_i18n::locale(),
                &(label),
                &(source)
            )
        ),
        Err(error) if is_rejected(&error) => {
            println!(
                "{}",
                nan_harness_i18n::messages::management_rejected_by_the_provider_through(
                    nan_harness_i18n::locale(),
                    &(label),
                    &(source)
                )
            );
        }
        Err(error) => println!(
            "{}",
            nan_harness_i18n::messages::management_could_not_be_verified_through(
                nan_harness_i18n::locale(),
                &(nan_harness_i18n::TerminalMessage::terminal_message(
                    &error,
                    nan_harness_i18n::locale()
                )),
                &(label),
                &(source)
            )
        ),
    }
}

pub(super) fn offer_configuration_refresh(
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
    interactive: bool,
) -> Result<(), CredentialError> {
    let manager = ConfigurationManager::from_environment().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    let configured = manager.configured_harnesses().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    let pen_configured = pen_desktop::persistent_configuration_exists().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    if configured.is_empty() && !pen_configured {
        return Ok(());
    }
    if !interactive
        || !prompt_yes_no(
            &nan_harness_i18n::messages::prompt_refresh_configurations(nan_harness_i18n::locale()),
            true,
        )?
    {
        println!("{}", nan_harness_i18n::messages::management_run_nanh_config_refresh_all_when_you_want_to_update_them(nan_harness_i18n::locale()));
        return Ok(());
    }
    for harness in configured {
        manager
            .configure(harness, config, models, None)
            .map_err(|error| {
                CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
            })?;
        println!(
            "{}",
            nan_harness_i18n::messages::management_updated_the_managed_configuration(
                nan_harness_i18n::locale(),
                &(harness)
            )
        );
    }
    if pen_configured
        && pen_desktop::refresh_persistent_with_config(config, models).map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?
    {
        println!(
            "{}",
            nan_harness_i18n::messages::management_updated_the_managed_pen_desktop_configuration(
                nan_harness_i18n::locale()
            )
        );
    }
    Ok(())
}

pub(super) fn prepare_logout(
    arguments: AuthLogoutArgs,
    interactive: bool,
) -> Result<bool, CredentialError> {
    let configuration_manager = ConfigurationManager::from_environment().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    let configured = configuration_manager
        .configured_harnesses()
        .map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?;
    let pen_configured = pen_desktop::persistent_configuration_exists().map_err(|error| {
        CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
    })?;
    if configured.is_empty() && !pen_configured {
        if !interactive && !arguments.yes {
            return Err(CredentialError::LogoutConfirmationRequired);
        }
        return Ok(true);
    }
    let remove_configs = if interactive && !arguments.yes {
        eprintln!(
            "{}", nan_harness_i18n::messages::management_the_saved_key_has_been_copied_into_managed_harness_configurations(nan_harness_i18n::locale(), &(configured.len() + usize::from(pen_configured))));
        eprintln!("{}", nan_harness_i18n::messages::management_1_remove_the_saved_key_and_all_managed_harness_configurations_recommended(nan_harness_i18n::locale()));
        eprintln!("{}", nan_harness_i18n::messages::management_2_remove_only_the_saved_key_and_keep_harness_configurations(nan_harness_i18n::locale()));
        eprintln!(
            "{}",
            nan_harness_i18n::messages::management_3_cancel(nan_harness_i18n::locale())
        );
        let Some(remove_configs) = prompt_logout_choice()? else {
            return Ok(false);
        };
        remove_configs
    } else {
        explicit_logout_mode(arguments)?
    };
    if remove_configs {
        configuration_manager.remove_all().map_err(|error| {
            CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
        })?;
        if pen_configured {
            pen_desktop::remove_persistent_configuration().map_err(|error| {
                CredentialError::ConfigurationOperation(nan_harness_i18n::ErrorCause::new(error))
            })?;
        }
        println!(
            "{}",
            nan_harness_i18n::messages::management_all_managed_harness_configurations_were_removed(
                nan_harness_i18n::locale()
            )
        );
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
