use super::super::{CredentialError, CredentialManager};
use super::management::{offer_configuration_refresh, prepare_logout, print_status};
use super::onboarding::prompt_and_store;
use super::prompt::prompt_api_key;
use crate::app::AuthCommand;
use nan_harness_runtime::ProcessEnvironment;
use std::env;

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
                println!(
                    "{}",
                    nan_harness_i18n::messages::dispatch_logout_cancelled(
                        nan_harness_i18n::locale()
                    )
                );
                return Ok(());
            }
            if manager.remove_saved()? {
                println!(
                    "{}",
                    nan_harness_i18n::messages::dispatch_saved_nan_api_key_removed(
                        nan_harness_i18n::locale()
                    )
                );
            } else {
                println!(
                    "{}",
                    nan_harness_i18n::messages::dispatch_no_saved_nan_api_key_is_configured(
                        nan_harness_i18n::locale()
                    )
                );
            }
            if env::var_os("NAN_API_KEY").is_some_and(|value| !value.is_empty()) {
                println!("{}", nan_harness_i18n::messages::dispatch_nan_api_key_remains_set_and_takes_precedence(nan_harness_i18n::locale()));
            }
        }
    }
    Ok(())
}
