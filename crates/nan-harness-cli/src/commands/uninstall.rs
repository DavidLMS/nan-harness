mod cleanup;
mod error;
mod installation;
mod prompting;
mod safety;

use crate::app::{RecordInstallationArgs, UninstallArgs};
use crate::commands::configuration::ConfigurationManager;
use crate::commands::credentials::CredentialManager;
use crate::commands::hermes_desktop;
use crate::commands::pen_desktop;
use crate::commands::persistence::{PersistenceManager, RemovalOutcome};
use crate::commands::search;
pub(crate) use error::UninstallError;

pub(crate) fn run(arguments: &UninstallArgs, interactive: bool) -> Result<(), UninstallError> {
    let manager = PersistenceManager::from_environment()?;
    let data_directory = manager.state_directory().to_path_buf();
    safety::validate_data_directory(&data_directory)?;
    safety::ensure_no_pending_desktop_session(&data_directory)?;
    search::ensure_no_active_search_sessions_for_uninstall()?;
    let installation = installation::resolve_installation(&data_directory)?;
    let integrations = manager.configured_integrations()?;
    let configuration_manager = ConfigurationManager::from_environment()?;
    let native_configurations = configuration_manager.configured_harnesses()?;
    let credential_manager = CredentialManager::for_data_directory(&data_directory)?;
    let has_saved_credential = credential_manager.has_saved()?;
    let has_chatgpt_profile = data_directory.join("chatgpt-desktop/profile").exists();
    let has_hermes_profile = hermes_desktop::persistent_profile_exists()?;
    let has_pen_configuration = pen_desktop::persistent_configuration_exists()?;

    if !arguments.yes {
        if !interactive {
            return Err(UninstallError::ConfirmationRequired);
        }
        let confirmed = {
            let mut input = std::io::stdin().lock();
            let mut output = std::io::stderr().lock();
            prompting::prompt(
                &installation,
                &data_directory,
                &integrations,
                &native_configurations,
                has_saved_credential,
                has_chatgpt_profile,
                has_hermes_profile,
                has_pen_configuration,
                &mut input,
                &mut output,
            )?
        };
        if !confirmed {
            println!(
                "{}",
                nan_harness_i18n::messages::uninstall_uninstall_cancelled(
                    nan_harness_i18n::locale()
                )
            );
            return Ok(());
        }
    }

    search::cleanup_owned_search_resources_for_uninstall()?;

    if has_hermes_profile && hermes_desktop::remove_persistent_profile()? {
        println!(
            "{}",
            nan_harness_i18n::messages::uninstall_hermes_cli_desktop_shared_nan_profile_removed(
                nan_harness_i18n::locale()
            )
        );
    }
    if has_pen_configuration && pen_desktop::remove_persistent_configuration()? {
        println!(
            "{}",
            nan_harness_i18n::messages::uninstall_nan_configuration_removed_from_pen_desktop(
                nan_harness_i18n::locale()
            )
        );
    }

    for (harness, outcome) in configuration_manager.remove_all()? {
        if outcome == RemovalOutcome::Removed {
            println!(
                "{}",
                nan_harness_i18n::messages::uninstall_nan_configuration_removed_from(
                    nan_harness_i18n::locale(),
                    &(harness)
                )
            );
        }
    }
    for integration in integrations {
        if manager.unpersist(integration)? == RemovalOutcome::Removed {
            println!(
                "{}",
                nan_harness_i18n::messages::uninstall_nan_provider_removed_from(
                    nan_harness_i18n::locale(),
                    &(integration)
                )
            );
        }
    }
    if credential_manager.remove_saved()? {
        println!(
            "{}",
            nan_harness_i18n::messages::uninstall_saved_nan_provider_api_key_removed(
                nan_harness_i18n::locale()
            )
        );
    }

    if !installation.remove_alias && installation.alias_path.exists() {
        eprintln!(
            "{}", nan_harness_i18n::messages::uninstall_warning_preserving_because_it_is_no_longer_managed_by_nan_harness(nan_harness_i18n::locale(), &(installation.alias_path.display())));
    }

    cleanup::remove_installation(&installation, &data_directory)?;
    Ok(())
}

pub(crate) fn record_installation(
    arguments: &RecordInstallationArgs,
) -> Result<(), UninstallError> {
    let manager = PersistenceManager::from_environment()?;
    let data_directory = manager.state_directory();
    safety::validate_data_directory(data_directory)?;
    installation::record_installation(arguments, data_directory)
}
