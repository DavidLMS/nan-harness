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
pub(crate) use error::UninstallError;

pub(crate) fn run(arguments: &UninstallArgs, interactive: bool) -> Result<(), UninstallError> {
    let manager = PersistenceManager::from_environment()?;
    let data_directory = manager.state_directory().to_path_buf();
    safety::validate_data_directory(&data_directory)?;
    safety::ensure_no_pending_desktop_session(&data_directory)?;
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
            println!("Uninstall cancelled.");
            return Ok(());
        }
    }

    if has_hermes_profile && hermes_desktop::remove_persistent_profile()? {
        println!("Hermes CLI/Desktop shared NaN profile removed.");
    }
    if has_pen_configuration && pen_desktop::remove_persistent_configuration()? {
        println!("NaN configuration removed from Pen Desktop.");
    }

    for (harness, outcome) in configuration_manager.remove_all()? {
        if outcome == RemovalOutcome::Removed {
            println!("NaN configuration removed from {harness}.");
        }
    }
    for integration in integrations {
        if manager.unpersist(integration)? == RemovalOutcome::Removed {
            println!("NaN provider removed from {integration}.");
        }
    }
    if credential_manager.remove_saved()? {
        println!("Saved NaN provider API key removed.");
    }

    if !installation.remove_alias && installation.alias_path.exists() {
        eprintln!(
            "warning: preserving '{}' because it is no longer managed by nan-harness",
            installation.alias_path.display()
        );
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
