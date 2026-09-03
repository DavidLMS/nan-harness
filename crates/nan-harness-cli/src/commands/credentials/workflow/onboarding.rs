use super::super::verification::{is_rejected, verify_cached, verify_models};
use super::super::{CredentialError, CredentialManager, CredentialSource, credential_fingerprint};
use super::ResolvedLaunchConfig;
use super::prompt::{prompt_api_key, prompt_yes_no};
use super::recovery::{existing_config, recover_rejected_credential, saved_config};
use nan_harness_core::{CodingModelProfile, SecretValue};
use nan_harness_runtime::{
    ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment, ResolvedConfig,
};

const NAN_GET_API_KEY_URL: &str = "https://nan.builders/";

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
pub(in crate::commands::credentials) async fn resolve_or_onboard_with(
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

pub(super) async fn prompt_and_store(
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

pub(in crate::commands::credentials) fn render_missing_credential_hint() -> String {
    format!("Get one at {NAN_GET_API_KEY_URL}")
}

pub(in crate::commands::credentials) fn render_first_harness_hint(
    announce_missing: bool,
) -> Option<&'static str> {
    announce_missing.then_some("Start your first harness with: nanh pi")
}

#[cfg(test)]
mod tests {
    use super::resolve_or_onboard_with;
    use crate::commands::credentials::{CredentialError, CredentialManager};
    use nan_harness_runtime::EnvironmentSource;

    struct EmptyEnvironment;

    impl EnvironmentSource for EmptyEnvironment {
        fn value(&self, _name: &str) -> Option<String> {
            None
        }
    }

    #[tokio::test]
    async fn non_interactive_onboarding_does_not_prompt_without_credentials() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());

        let result = resolve_or_onboard_with(&EmptyEnvironment, &manager, None, false, || {
            panic!("non-interactive onboarding must not prompt")
        })
        .await;

        assert!(matches!(result, Err(CredentialError::MissingCredential)));
    }
}
