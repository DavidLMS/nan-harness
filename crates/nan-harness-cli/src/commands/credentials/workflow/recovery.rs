use super::super::verification::{is_rejected, verify_models};
use super::super::{CredentialError, CredentialManager, CredentialSource};
use super::ResolvedLaunchConfig;
use super::onboarding::prompt_and_store;
use super::prompt::{prompt_api_key, prompt_yes_no};
use crate::commands::configuration::ConfigurationManager;
use crate::commands::pen_desktop;
use nan_harness_runtime::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};

pub(super) fn existing_config(
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

pub(super) async fn recover_rejected_credential(
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

pub(super) fn saved_config(
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

#[cfg(test)]
mod tests {
    use super::existing_config;
    use crate::commands::credentials::{CredentialManager, CredentialSource};
    use nan_harness_runtime::EnvironmentSource;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestEnvironment(BTreeMap<String, String>);

    impl EnvironmentSource for TestEnvironment {
        fn value(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    #[test]
    fn environment_credentials_take_precedence_over_saved_credentials() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());
        manager
            .save("nan-saved-test-key")
            .expect("saved test credential should persist");
        let environment = TestEnvironment(BTreeMap::from([(
            "NAN_API_KEY".to_owned(),
            "nan-environment-test-key".to_owned(),
        )]));

        let (config, source) = existing_config(&environment, &manager, None)
            .expect("credential resolution should succeed")
            .expect("an environment credential should resolve");

        assert_eq!(source, CredentialSource::Environment);
        config
            .secrets
            .with_secret(&config.provider_credential_ref, |value| {
                assert_eq!(value, "nan-environment-test-key");
            })
            .expect("resolved environment credential should exist");
    }

    #[test]
    fn saved_credentials_are_used_when_the_environment_is_empty() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());
        manager
            .save("nan-saved-test-key")
            .expect("saved test credential should persist");

        let (config, source) = existing_config(&TestEnvironment::default(), &manager, None)
            .expect("credential resolution should succeed")
            .expect("a saved credential should resolve");

        assert_eq!(source, CredentialSource::PrivateFile);
        config
            .secrets
            .with_secret(&config.provider_credential_ref, |value| {
                assert_eq!(value, "nan-saved-test-key");
            })
            .expect("resolved saved credential should exist");
    }
}
