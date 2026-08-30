use crate::app::AuthCommand;
use crate::commands::configuration::ConfigurationManager;
use crate::commands::persistence::{
    PersistenceError, config_directory, discover_models, write_private_file,
};
use keyring::{Entry, Error as KeyringError};
use nan_harness_core::{CodingModelProfile, SecretError, SecretValue};
use nan_harness_private_fs::{PrivateFileReadStatus, open_private_read};
use nan_harness_runtime::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const CREDENTIAL_BACKEND_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CREDENTIAL_BACKEND";
const CREDENTIAL_FILE_NAME: &str = "nan-api-key";
const CREDENTIAL_RECEIPT_FILE_NAME: &str = "credential.json";
const CREDENTIAL_RECEIPT_SCHEMA_VERSION: u8 = 1;
const KEYRING_SERVICE: &str = "nan-harness";
const KEYRING_USER: &str = "nan-api-key";
const NAN_GET_API_KEY_URL: &str = "https://nan.builders/";
const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(10);
const VERIFICATION_CACHE_TTL: Duration = Duration::from_hours(1);
const VERIFICATION_CACHE_FILE_NAME: &str = "credential-verification.json";
const VERIFICATION_CACHE_SCHEMA_VERSION: u8 = 1;
const SAVED_KEY_REPAIR_WARNING: &str =
    "warning: restored private permissions on the saved NaN API key.";
const CREDENTIAL_METADATA_REPAIR_WARNING: &str =
    "warning: restored private permissions on NaN credential metadata.";
const VERIFICATION_RECEIPT_REPAIR_WARNING: &str =
    "warning: restored private permissions on the NaN verification receipt.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialSource {
    Environment,
    SystemKeyring,
    PrivateFile,
}

#[derive(Debug)]
pub(crate) struct ResolvedLaunchConfig {
    pub(crate) config: ResolvedConfig,
    pub(crate) model_catalog: Option<Vec<CodingModelProfile>>,
}

impl std::fmt::Display for CredentialSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Environment => "NAN_API_KEY",
            Self::SystemKeyring => "the system credential store",
            Self::PrivateFile => "the private nan-harness credential file",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendPreference {
    Auto,
    Keyring,
    File,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum StoredBackend {
    SystemKeyring,
    PrivateFile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CredentialReceipt {
    schema_version: u8,
    backend: StoredBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationReceipt {
    schema_version: u8,
    provider_base_url: String,
    credential_fingerprint: String,
    verified_at_unix_seconds: u64,
}

#[derive(Debug)]
pub(crate) struct CredentialManager {
    credential_path: PathBuf,
    receipt_path: PathBuf,
    preference: BackendPreference,
}

impl CredentialManager {
    pub(crate) fn from_environment() -> Result<Self, CredentialError> {
        let directory = config_directory().ok_or(CredentialError::MissingConfigDirectory)?;
        Self::with_preference(directory, backend_preference()?)
    }

    pub(crate) fn for_data_directory(
        directory: impl Into<PathBuf>,
    ) -> Result<Self, CredentialError> {
        Self::with_preference(directory, backend_preference()?)
    }

    fn with_preference(
        directory: impl Into<PathBuf>,
        preference: BackendPreference,
    ) -> Result<Self, CredentialError> {
        let directory = directory.into();
        if !directory.is_absolute() {
            return Err(CredentialError::InvalidConfigDirectory(directory));
        }
        Ok(Self {
            credential_path: directory.join(CREDENTIAL_FILE_NAME),
            receipt_path: directory.join(CREDENTIAL_RECEIPT_FILE_NAME),
            preference,
        })
    }

    pub(crate) fn has_saved(&self) -> Result<bool, CredentialError> {
        Ok(self.receipt()?.is_some())
    }

    pub(crate) fn load(&self) -> Result<Option<(SecretValue, CredentialSource)>, CredentialError> {
        if let Some(receipt) = self.receipt()? {
            return match receipt.backend {
                StoredBackend::SystemKeyring => Ok(Self::read_system_keyring()?
                    .map(|secret| (secret, CredentialSource::SystemKeyring))),
                StoredBackend::PrivateFile => Ok(self
                    .read_private_file()?
                    .map(|secret| (secret, CredentialSource::PrivateFile))),
            };
        }

        match self.preference {
            BackendPreference::File => self
                .read_private_file()
                .map(|secret| secret.map(|value| (value, CredentialSource::PrivateFile))),
            BackendPreference::Keyring => Self::read_system_keyring()
                .map(|secret| secret.map(|value| (value, CredentialSource::SystemKeyring))),
            BackendPreference::Auto => match Self::read_system_keyring() {
                Ok(Some(secret)) => Ok(Some((secret, CredentialSource::SystemKeyring))),
                Ok(None) | Err(_) => self
                    .read_private_file()
                    .map(|secret| secret.map(|value| (value, CredentialSource::PrivateFile))),
            },
        }
    }

    pub(crate) fn save(&self, api_key: &str) -> Result<CredentialSource, CredentialError> {
        let existing_backend = self.receipt()?.map(|receipt| receipt.backend);
        match self.preference {
            BackendPreference::File => self.save_private_file(api_key),
            BackendPreference::Keyring => self.save_system_keyring(api_key),
            BackendPreference::Auto if existing_backend == Some(StoredBackend::SystemKeyring) => {
                self.save_system_keyring(api_key)
            }
            BackendPreference::Auto => match self.save_system_keyring(api_key) {
                Ok(source) => Ok(source),
                Err(_) => self.save_private_file(api_key),
            },
        }
    }

    pub(crate) fn remove_saved(&self) -> Result<bool, CredentialError> {
        let Some(receipt) = self.receipt()? else {
            return Ok(false);
        };
        match receipt.backend {
            StoredBackend::SystemKeyring => Self::delete_system_keyring()?,
            StoredBackend::PrivateFile => remove_file_if_present(&self.credential_path)?,
        }
        remove_file_if_present(&self.receipt_path)?;
        Ok(true)
    }

    fn save_system_keyring(&self, api_key: &str) -> Result<CredentialSource, CredentialError> {
        let receipt_is_current = self
            .receipt()?
            .is_some_and(|receipt| receipt.backend == StoredBackend::SystemKeyring);
        let entry = keyring_entry()?;
        entry
            .set_password(api_key)
            .map_err(CredentialError::Keyring)?;
        if receipt_is_current {
            return Ok(CredentialSource::SystemKeyring);
        }
        let receipt = CredentialReceipt {
            schema_version: CREDENTIAL_RECEIPT_SCHEMA_VERSION,
            backend: StoredBackend::SystemKeyring,
        };
        if let Err(error) = self.save_receipt(receipt) {
            let _ = entry.delete_credential();
            return Err(error);
        }
        remove_file_if_present(&self.credential_path)?;
        Ok(CredentialSource::SystemKeyring)
    }

    fn save_private_file(&self, api_key: &str) -> Result<CredentialSource, CredentialError> {
        let existing_backend = self.receipt()?.map(|receipt| receipt.backend);
        write_private_file(&self.credential_path, api_key.as_bytes(), None)?;
        let receipt = CredentialReceipt {
            schema_version: CREDENTIAL_RECEIPT_SCHEMA_VERSION,
            backend: StoredBackend::PrivateFile,
        };
        if existing_backend != Some(StoredBackend::PrivateFile)
            && let Err(error) = self.save_receipt(receipt)
        {
            let _ = fs::remove_file(&self.credential_path);
            return Err(error);
        }
        Ok(CredentialSource::PrivateFile)
    }

    fn read_system_keyring() -> Result<Option<SecretValue>, CredentialError> {
        match keyring_entry()?.get_password() {
            Ok(api_key) => SecretValue::new(api_key)
                .map(Some)
                .map_err(CredentialError::Secret),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(CredentialError::Keyring(error)),
        }
    }

    fn read_private_file(&self) -> Result<Option<SecretValue>, CredentialError> {
        let Some(mut file) =
            open_private_file_for_read(&self.credential_path, SAVED_KEY_REPAIR_WARNING)?
        else {
            return Ok(None);
        };
        let mut api_key = String::new();
        file.read_to_string(&mut api_key)
            .map_err(|source| CredentialError::ReadFile {
                path: self.credential_path.clone(),
                source,
            })?;
        SecretValue::new(api_key)
            .map(Some)
            .map_err(CredentialError::Secret)
    }

    fn delete_system_keyring() -> Result<(), CredentialError> {
        match keyring_entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(CredentialError::Keyring(error)),
        }
    }

    fn receipt(&self) -> Result<Option<CredentialReceipt>, CredentialError> {
        let Some(mut file) =
            open_private_file_for_read(&self.receipt_path, CREDENTIAL_METADATA_REPAIR_WARNING)?
        else {
            return Ok(None);
        };
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|source| CredentialError::ReadFile {
                path: self.receipt_path.clone(),
                source,
            })?;
        let receipt: CredentialReceipt =
            serde_json::from_slice(&contents).map_err(CredentialError::ParseReceipt)?;
        if receipt.schema_version != CREDENTIAL_RECEIPT_SCHEMA_VERSION {
            return Err(CredentialError::UnsupportedReceiptSchema(
                receipt.schema_version,
            ));
        }
        Ok(Some(receipt))
    }

    fn save_receipt(&self, receipt: CredentialReceipt) -> Result<(), CredentialError> {
        let payload =
            serde_json::to_vec_pretty(&receipt).map_err(CredentialError::SerializeReceipt)?;
        write_private_file(&self.receipt_path, &payload, None).map_err(CredentialError::State)
    }

    #[cfg(test)]
    fn file_backend(directory: impl Into<PathBuf>) -> Self {
        Self::with_preference(directory, BackendPreference::File)
            .expect("test credential directory should be valid")
    }
}

fn open_private_file_for_read(
    path: &Path,
    repaired_warning: &'static str,
) -> Result<Option<File>, CredentialError> {
    match open_private_read(path) {
        Ok((file, status)) => {
            if let Some(warning) = private_file_repair_warning(status, repaired_warning) {
                eprintln!("{warning}");
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CredentialError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn private_file_repair_warning(
    status: PrivateFileReadStatus,
    repaired_warning: &'static str,
) -> Option<&'static str> {
    (status == PrivateFileReadStatus::Repaired).then_some(repaired_warning)
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
                    "Other managed harness configurations still contain the previous key; update them with `nan config --refresh-all`."
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
async fn resolve_or_onboard_with(
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

fn render_missing_credential_hint() -> String {
    format!("Get one at {NAN_GET_API_KEY_URL}")
}

fn render_first_harness_hint(announce_missing: bool) -> Option<&'static str> {
    announce_missing.then_some("Start your first harness with: nan pi")
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

pub(crate) async fn verify(config: &ResolvedConfig) -> Result<(), CredentialError> {
    verify_models(config).await.map(|_| ())
}

async fn verify_models(
    config: &ResolvedConfig,
) -> Result<Vec<CodingModelProfile>, CredentialError> {
    match tokio::time::timeout(VERIFICATION_TIMEOUT, discover_models(config)).await {
        Ok(Ok(models)) => Ok(models),
        Ok(Err(error)) => Err(CredentialError::Verification(error)),
        Err(_) => Err(CredentialError::VerificationTimeout),
    }
}

async fn verify_cached(
    config: &ResolvedConfig,
) -> Result<Option<Vec<CodingModelProfile>>, CredentialError> {
    let fingerprint = credential_fingerprint(config)?;
    let cache_path = verification_cache_path()?;
    verify_cached_at(config, &cache_path, &fingerprint).await
}

async fn verify_cached_at(
    config: &ResolvedConfig,
    cache_path: &Path,
    fingerprint: &str,
) -> Result<Option<Vec<CodingModelProfile>>, CredentialError> {
    if verification_cache_is_current(cache_path, &config.provider_base_url, fingerprint)? {
        return Ok(None);
    }
    let models = verify_models(config).await?;
    let receipt = VerificationReceipt {
        schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
        provider_base_url: config.provider_base_url.clone(),
        credential_fingerprint: fingerprint.to_owned(),
        verified_at_unix_seconds: unix_time()?,
    };
    let payload = serde_json::to_vec_pretty(&receipt)
        .map_err(CredentialError::SerializeVerificationReceipt)?;
    write_private_file(cache_path, &payload, None)?;
    Ok(Some(models))
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
        if !configuration_manager
            .configured_harnesses()
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?
            .is_empty()
        {
            eprintln!(
                "Managed harness configurations still contain the previous key; update them with `nan config --refresh-all`."
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
    println!(
        "Managed harness configurations: {} total, {} needing attention.",
        configured.len(),
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
    if configured.is_empty() {
        return Ok(());
    }
    if !interactive
        || !prompt_yes_no(
            "Update all harness configurations managed by nan-harness with this key? [Y/n] ",
            true,
        )?
    {
        println!("Run `nan config --refresh-all` when you want to update them.");
        return Ok(());
    }
    for harness in configured {
        manager
            .configure(harness, config, models)
            .map_err(|error| CredentialError::ConfigurationOperation(error.to_string()))?;
        println!("Updated the managed {harness} configuration.");
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
    if configured.is_empty() {
        if !interactive && !arguments.yes {
            return Err(CredentialError::LogoutConfirmationRequired);
        }
        return Ok(true);
    }
    let remove_configs = if interactive && !arguments.yes {
        eprintln!(
            "The saved key has been copied into {} managed harness configurations.",
            configured.len()
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

fn credential_fingerprint(config: &ResolvedConfig) -> Result<String, CredentialError> {
    config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| {
            let digest = Sha256::digest(value.as_bytes());
            hex(&digest)
        })
        .map_err(CredentialError::Secret)
}

fn verification_cache_path() -> Result<PathBuf, CredentialError> {
    config_directory()
        .map(|directory| directory.join(VERIFICATION_CACHE_FILE_NAME))
        .ok_or(CredentialError::MissingConfigDirectory)
}

fn verification_cache_is_current(
    path: &Path,
    provider_base_url: &str,
    fingerprint: &str,
) -> Result<bool, CredentialError> {
    let Some(mut file) = open_private_file_for_read(path, VERIFICATION_RECEIPT_REPAIR_WARNING)?
    else {
        return Ok(false);
    };
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|source| CredentialError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
    let receipt: VerificationReceipt =
        serde_json::from_slice(&contents).map_err(CredentialError::ParseVerificationReceipt)?;
    if receipt.schema_version != VERIFICATION_CACHE_SCHEMA_VERSION {
        return Ok(false);
    }
    let age = unix_time()?.saturating_sub(receipt.verified_at_unix_seconds);
    Ok(receipt.provider_base_url == provider_base_url
        && receipt.credential_fingerprint == fingerprint
        && age < VERIFICATION_CACHE_TTL.as_secs())
}

fn unix_time() -> Result<u64, CredentialError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(CredentialError::SystemTime)
}

fn is_rejected(error: &CredentialError) -> bool {
    matches!(
        error,
        CredentialError::Verification(PersistenceError::ModelDiscoveryStatus(401 | 403))
    )
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

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn prompt_api_key() -> Result<SecretValue, CredentialError> {
    let api_key = rpassword::prompt_password("NaN API key (input hidden): ")
        .map_err(CredentialError::Prompt)?;
    SecretValue::new(api_key).map_err(CredentialError::Secret)
}

fn keyring_entry() -> Result<Entry, CredentialError> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(CredentialError::Keyring)
}

fn backend_preference() -> Result<BackendPreference, CredentialError> {
    match env::var(CREDENTIAL_BACKEND_ENVIRONMENT_VARIABLE) {
        Ok(value) if value.eq_ignore_ascii_case("auto") => Ok(BackendPreference::Auto),
        Ok(value) if value.eq_ignore_ascii_case("keyring") => Ok(BackendPreference::Keyring),
        Ok(value) if value.eq_ignore_ascii_case("file") => Ok(BackendPreference::File),
        Ok(value) => Err(CredentialError::InvalidBackend(value)),
        Err(env::VarError::NotPresent) => Ok(BackendPreference::Auto),
        Err(env::VarError::NotUnicode(_)) => Err(CredentialError::NonUnicodeBackend),
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), CredentialError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CredentialError::RemoveFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[derive(Debug, Error)]
pub(crate) enum CredentialError {
    #[error(
        "no NaN API key is configured; run `nan auth login` in an interactive terminal or set NAN_API_KEY"
    )]
    MissingCredential,
    #[error(
        "no API key is saved by nan-harness; run `nan auth login` interactively before using `nan config`"
    )]
    MissingSavedCredential,
    #[error("`nan auth login` requires an interactive terminal")]
    InteractiveLoginRequired,
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("nan-harness configuration directory '{}' must be absolute", .0.display())]
    InvalidConfigDirectory(PathBuf),
    #[error("NAN_HARNESS_CREDENTIAL_BACKEND must be auto, keyring, or file; received '{0}'")]
    InvalidBackend(String),
    #[error("NAN_HARNESS_CREDENTIAL_BACKEND is not valid Unicode")]
    NonUnicodeBackend,
    #[error("could not read the hidden API key: {0}")]
    Prompt(std::io::Error),
    #[error("could not access the system credential store: {0}")]
    Keyring(KeyringError),
    #[error("could not read credential file '{}': {source}", path.display())]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove credential file '{}': {source}", path.display())]
    RemoveFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("credential receipt is not valid JSON: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("credential receipt schema {0} is not supported")]
    UnsupportedReceiptSchema(u8),
    #[error("could not serialize the credential receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("credential verification receipt is not valid JSON: {0}")]
    ParseVerificationReceipt(serde_json::Error),
    #[error("could not serialize the credential verification receipt: {0}")]
    SerializeVerificationReceipt(serde_json::Error),
    #[error("the system clock is earlier than the Unix epoch: {0}")]
    SystemTime(std::time::SystemTimeError),
    #[error("`nan auth logout` requires --yes in a non-interactive terminal")]
    LogoutConfirmationRequired,
    #[error(
        "non-interactive logout with managed configurations requires --yes and exactly one of --remove-configs or --keep-configs"
    )]
    LogoutModeRequired,
    #[error("logout choice must be 1, 2, or 3")]
    InvalidLogoutChoice,
    #[error("could not update managed harness configurations: {0}")]
    ConfigurationOperation(String),
    #[error("could not store the NaN API key: {0}")]
    State(#[from] PersistenceError),
    #[error("the NaN API key is invalid: {0}")]
    Secret(SecretError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("could not verify the NaN API key: {0}")]
    Verification(PersistenceError),
    #[error("NaN API key verification timed out after 10 seconds")]
    VerificationTimeout,
}

impl CredentialError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::MissingCredential
            | Self::MissingSavedCredential
            | Self::InteractiveLoginRequired => "NH-CREDENTIAL-001",
            Self::Prompt(_) | Self::Secret(_) => "NH-CREDENTIAL-002",
            Self::Verification(_) | Self::VerificationTimeout => "NH-CREDENTIAL-004",
            Self::Config(error) => error.code(),
            Self::MissingConfigDirectory
            | Self::InvalidConfigDirectory(_)
            | Self::InvalidBackend(_)
            | Self::NonUnicodeBackend
            | Self::Keyring(_)
            | Self::ReadFile { .. }
            | Self::RemoveFile { .. }
            | Self::ParseReceipt(_)
            | Self::UnsupportedReceiptSchema(_)
            | Self::SerializeReceipt(_)
            | Self::ParseVerificationReceipt(_)
            | Self::SerializeVerificationReceipt(_)
            | Self::SystemTime(_)
            | Self::LogoutConfirmationRequired
            | Self::LogoutModeRequired
            | Self::InvalidLogoutChoice
            | Self::ConfigurationOperation(_)
            | Self::State(_) => "NH-CREDENTIAL-003",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CredentialManager, CredentialSource, VERIFICATION_CACHE_SCHEMA_VERSION,
        VERIFICATION_CACHE_TTL, VerificationReceipt, is_rejected, render_first_harness_hint,
        render_missing_credential_hint, resolve_or_onboard_with, verification_cache_is_current,
        verify_cached_at,
    };
    use crate::commands::persistence::PersistenceError;
    use nan_harness_core::SecretValue;
    use nan_harness_runtime::{
        ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    };
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestEnvironment(BTreeMap<String, String>);

    impl EnvironmentSource for TestEnvironment {
        fn value(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    #[test]
    fn missing_credential_hint_includes_api_url_once() {
        let hint = render_missing_credential_hint();

        assert_eq!(hint, "Get one at https://nan.builders/");
        assert_eq!(hint.matches("https://nan.builders/").count(), 1);
    }

    #[test]
    fn first_harness_hint_only_renders_for_initial_onboarding() {
        assert_eq!(
            render_first_harness_hint(true),
            Some("Start your first harness with: nan pi")
        );
        assert_eq!(render_first_harness_hint(false), None);
    }

    #[tokio::test]
    async fn prompted_credentials_are_verified_saved_and_reused() {
        let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
            .await
            .expect("scripted provider should start");
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());
        let environment = TestEnvironment::default();

        let config = resolve_or_onboard_with(
            &environment,
            &manager,
            Some(provider.base_url().to_owned()),
            true,
            || SecretValue::new("nan-test-key").map_err(super::CredentialError::Secret),
        )
        .await
        .expect("interactive onboarding should succeed");
        config
            .secrets
            .with_secret(&config.provider_credential_ref, |value| {
                assert_eq!(value, "nan-test-key");
            })
            .expect("resolved credential should exist");

        let reused = resolve_or_onboard_with(
            &environment,
            &manager,
            Some(provider.base_url().to_owned()),
            false,
            || panic!("a saved credential must not prompt"),
        )
        .await
        .expect("saved credential should resolve non-interactively");
        reused
            .secrets
            .with_secret(&reused.provider_credential_ref, |value| {
                assert_eq!(value, "nan-test-key");
            })
            .expect("reused credential should exist");
        assert_eq!(
            manager
                .load()
                .expect("saved credential should load")
                .map(|(_, source)| source),
            Some(CredentialSource::PrivateFile)
        );
        assert!(
            manager
                .remove_saved()
                .expect("credential should be removed")
        );
        assert!(!manager.has_saved().expect("receipt should be removed"));

        provider.shutdown().await.expect("provider should stop");
    }

    #[tokio::test]
    async fn current_verification_receipt_skips_model_discovery() {
        let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
            .await
            .expect("scripted provider should start");
        let config = ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: Some(provider.base_url().to_owned()),
                nan_api_key: Some(
                    SecretValue::new("nan-test-key").expect("test key should be valid"),
                ),
            },
        )
        .expect("test configuration should resolve");
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let cache_path = directory.path().join("credential-verification.json");
        let fingerprint = super::credential_fingerprint(&config)
            .expect("test credential should have a fingerprint");
        std::fs::write(
            &cache_path,
            serde_json::to_vec(&VerificationReceipt {
                schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
                provider_base_url: config.provider_base_url.clone(),
                credential_fingerprint: fingerprint.clone(),
                verified_at_unix_seconds: super::unix_time()
                    .expect("system time should be available"),
            })
            .expect("receipt should serialize"),
        )
        .expect("receipt should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o644))
                .expect("verification receipt should be made permissive");
        }
        #[cfg(windows)]
        nan_harness_test_support::windows_acl::make_permissive_file(&cache_path)
            .expect("verification receipt DACL should be made permissive");

        let model_catalog = verify_cached_at(&config, &cache_path, &fingerprint)
            .await
            .expect("current receipt should verify");
        assert!(model_catalog.is_none());
        assert_eq!(provider.model_requests(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&cache_path)
                    .expect("verification receipt metadata should exist")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        #[cfg(windows)]
        nan_harness_test_support::windows_acl::assert_private_file(&cache_path)
            .expect("verification receipt DACL should be repaired");
        provider.shutdown().await.expect("provider should stop");
    }

    #[tokio::test]
    async fn missing_and_expired_receipts_return_fresh_model_catalogs() {
        let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
            .await
            .expect("scripted provider should start");
        let config = ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: Some(provider.base_url().to_owned()),
                nan_api_key: Some(
                    SecretValue::new("nan-test-key").expect("test key should be valid"),
                ),
            },
        )
        .expect("test configuration should resolve");
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let fingerprint = super::credential_fingerprint(&config)
            .expect("test credential should have a fingerprint");

        let missing_path = directory.path().join("missing-verification.json");
        let missing_models = verify_cached_at(&config, &missing_path, &fingerprint)
            .await
            .expect("missing receipt should trigger verification")
            .expect("fresh verification should return its model catalog");
        assert!(!missing_models.is_empty());
        assert_eq!(provider.model_requests(), 1);
        assert!(
            verification_cache_is_current(&missing_path, &config.provider_base_url, &fingerprint)
                .expect("renewed receipt should load")
        );

        let expired_path = directory.path().join("expired-verification.json");
        std::fs::write(
            &expired_path,
            serde_json::to_vec(&VerificationReceipt {
                schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
                provider_base_url: config.provider_base_url.clone(),
                credential_fingerprint: fingerprint.clone(),
                verified_at_unix_seconds: super::unix_time()
                    .expect("system time should be available")
                    .saturating_sub(VERIFICATION_CACHE_TTL.as_secs()),
            })
            .expect("receipt should serialize"),
        )
        .expect("expired receipt should be written");
        let expired_models = verify_cached_at(&config, &expired_path, &fingerprint)
            .await
            .expect("expired receipt should trigger verification")
            .expect("fresh verification should return its model catalog");
        assert!(!expired_models.is_empty());
        assert_eq!(provider.model_requests(), 2);
        assert!(
            verification_cache_is_current(&expired_path, &config.provider_base_url, &fingerprint)
                .expect("renewed receipt should load")
        );
        provider.shutdown().await.expect("provider should stop");
    }

    #[test]
    fn private_credentials_use_owner_only_permissions() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());
        manager
            .save("nan-test-key")
            .expect("credential should be saved");

        let credential_path = directory.path().join("nan-api-key");
        let receipt_path = directory.path().join("credential.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = |path: &std::path::Path| {
                std::fs::metadata(path)
                    .expect("private credential file should exist")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode(&credential_path), 0o600);
            assert_eq!(mode(&receipt_path), 0o600);
            std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o644))
                .expect("credential should be made permissive");
            std::fs::set_permissions(&receipt_path, std::fs::Permissions::from_mode(0o644))
                .expect("receipt should be made permissive");
        }
        #[cfg(windows)]
        {
            use nan_harness_test_support::windows_acl::{
                assert_private_file, make_permissive_file,
            };

            assert_private_file(&credential_path)
                .expect("credential should have a private protected DACL");
            assert_private_file(&receipt_path)
                .expect("receipt should have a private protected DACL");

            make_permissive_file(&credential_path)
                .expect("credential ACL should be made permissive");
            make_permissive_file(&receipt_path).expect("receipt ACL should be made permissive");
        }

        let (api_key, source) = manager
            .load()
            .expect("permissive credentials should be repaired")
            .expect("saved credential should remain available");
        assert_eq!(source, CredentialSource::PrivateFile);
        api_key.with_secret(|value| assert_eq!(value, "nan-test-key"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = |path: &std::path::Path| {
                std::fs::metadata(path)
                    .expect("repaired credential file should exist")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(mode(&credential_path), 0o600);
            assert_eq!(mode(&receipt_path), 0o600);
        }
        #[cfg(windows)]
        {
            use nan_harness_test_support::windows_acl::assert_private_file;

            assert_private_file(&credential_path)
                .expect("credential read should restore a private protected DACL");
            assert_private_file(&receipt_path)
                .expect("receipt read should restore a private protected DACL");
        }
    }

    #[test]
    fn missing_private_credentials_remain_absent() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());

        assert!(
            manager
                .load()
                .expect("missing credentials should not fail")
                .is_none()
        );
    }

    #[test]
    fn permission_repair_warnings_are_fixed_and_path_free() {
        for (warning, expected) in [
            (
                super::SAVED_KEY_REPAIR_WARNING,
                "warning: restored private permissions on the saved NaN API key.",
            ),
            (
                super::CREDENTIAL_METADATA_REPAIR_WARNING,
                "warning: restored private permissions on NaN credential metadata.",
            ),
            (
                super::VERIFICATION_RECEIPT_REPAIR_WARNING,
                "warning: restored private permissions on the NaN verification receipt.",
            ),
        ] {
            assert_eq!(
                super::private_file_repair_warning(
                    nan_harness_private_fs::PrivateFileReadStatus::AlreadyPrivate,
                    warning,
                ),
                None,
                "already-private fixtures must not emit a repair warning"
            );
            assert_eq!(
                super::private_file_repair_warning(
                    nan_harness_private_fs::PrivateFileReadStatus::Repaired,
                    warning,
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn verification_cache_is_scoped_to_endpoint_key_and_one_hour() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("credential-verification.json");
        let now = super::unix_time().expect("system time should be available");
        let write = |verified_at_unix_seconds| {
            std::fs::write(
                &path,
                serde_json::to_vec(&VerificationReceipt {
                    schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
                    provider_base_url: "https://api.nan.test/v1".to_owned(),
                    credential_fingerprint: "fingerprint-a".to_owned(),
                    verified_at_unix_seconds,
                })
                .expect("verification receipt should serialize"),
            )
            .expect("verification receipt should be written");
        };

        write(now);
        assert!(
            verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-a")
                .expect("fresh cache should load")
        );
        assert!(
            !verification_cache_is_current(&path, "https://other.nan.test/v1", "fingerprint-a")
                .expect("endpoint mismatch should load")
        );
        assert!(
            !verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-b")
                .expect("credential mismatch should load")
        );

        write(now.saturating_sub(VERIFICATION_CACHE_TTL.as_secs()));
        assert!(
            !verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-a")
                .expect("expired cache should load")
        );
    }

    #[test]
    fn only_provider_authentication_statuses_trigger_key_recovery() {
        for status in [401, 403] {
            assert!(is_rejected(&super::CredentialError::Verification(
                PersistenceError::ModelDiscoveryStatus(status)
            )));
        }
        for status in [400, 408, 429, 500, 503] {
            assert!(!is_rejected(&super::CredentialError::Verification(
                PersistenceError::ModelDiscoveryStatus(status)
            )));
        }
    }
}
