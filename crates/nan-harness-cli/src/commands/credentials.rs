use crate::app::AuthCommand;
use crate::commands::persistence::{
    PersistenceError, config_directory, discover_models, write_private_file,
};
use keyring::{Entry, Error as KeyringError};
use nan_harness_core::{SecretError, SecretValue};
use nan_harness_runtime::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const CREDENTIAL_BACKEND_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CREDENTIAL_BACKEND";
const CREDENTIAL_FILE_NAME: &str = "nan-api-key";
const CREDENTIAL_RECEIPT_FILE_NAME: &str = "credential.json";
const CREDENTIAL_RECEIPT_SCHEMA_VERSION: u8 = 1;
const KEYRING_SERVICE: &str = "nan-harness";
const KEYRING_USER: &str = "nan-api-key";
const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialSource {
    Environment,
    SystemKeyring,
    PrivateFile,
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

    fn save(&self, api_key: &str) -> Result<CredentialSource, CredentialError> {
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
        match fs::read_to_string(&self.credential_path) {
            Ok(api_key) => SecretValue::new(api_key)
                .map(Some)
                .map_err(CredentialError::Secret),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(CredentialError::ReadFile {
                path: self.credential_path.clone(),
                source,
            }),
        }
    }

    fn delete_system_keyring() -> Result<(), CredentialError> {
        match keyring_entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(CredentialError::Keyring(error)),
        }
    }

    fn receipt(&self) -> Result<Option<CredentialReceipt>, CredentialError> {
        match fs::read(&self.receipt_path) {
            Ok(contents) => {
                let receipt: CredentialReceipt =
                    serde_json::from_slice(&contents).map_err(CredentialError::ParseReceipt)?;
                if receipt.schema_version != CREDENTIAL_RECEIPT_SCHEMA_VERSION {
                    return Err(CredentialError::UnsupportedReceiptSchema(
                        receipt.schema_version,
                    ));
                }
                Ok(Some(receipt))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(CredentialError::ReadFile {
                path: self.receipt_path.clone(),
                source,
            }),
        }
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

pub(crate) async fn run(command: AuthCommand, interactive: bool) -> Result<(), CredentialError> {
    let manager = CredentialManager::from_environment()?;
    match command {
        AuthCommand::Login => {
            if !interactive {
                return Err(CredentialError::InteractiveLoginRequired);
            }
            let (config, _) =
                prompt_and_store(&ProcessEnvironment, &manager, None, false, prompt_api_key)
                    .await?;
            drop(config);
        }
        AuthCommand::Status => match existing_config(&ProcessEnvironment, &manager, None)? {
            Some((_, source)) => println!("NaN API key: configured through {source}."),
            None => println!("NaN API key: not configured."),
        },
        AuthCommand::Logout => {
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

pub(crate) fn resolve_existing_config(
    provider_base_url: Option<String>,
) -> Result<Option<ResolvedConfig>, CredentialError> {
    let manager = CredentialManager::from_environment()?;
    existing_config(&ProcessEnvironment, &manager, provider_base_url)
        .map(|resolved| resolved.map(|(config, _)| config))
}

pub(crate) async fn resolve_or_onboard(
    provider_base_url: Option<String>,
    interactive: bool,
) -> Result<ResolvedConfig, CredentialError> {
    let manager = CredentialManager::from_environment()?;
    resolve_or_onboard_with(
        &ProcessEnvironment,
        &manager,
        provider_base_url,
        interactive,
        prompt_api_key,
    )
    .await
}

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
        .map(|(config, _)| config)
}

async fn prompt_and_store(
    environment: &impl EnvironmentSource,
    manager: &CredentialManager,
    provider_base_url: Option<String>,
    announce_missing: bool,
    prompt: impl FnOnce() -> Result<SecretValue, CredentialError>,
) -> Result<(ResolvedConfig, CredentialSource), CredentialError> {
    if announce_missing {
        eprintln!("NAN_API_KEY is not configured.");
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
    verify(&config).await?;
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
    Ok((config, source))
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

async fn verify(config: &ResolvedConfig) -> Result<(), CredentialError> {
    match tokio::time::timeout(VERIFICATION_TIMEOUT, discover_models(config)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(CredentialError::Verification(error)),
        Err(_) => Err(CredentialError::VerificationTimeout),
    }
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
            Self::MissingCredential | Self::InteractiveLoginRequired => "NH-CREDENTIAL-001",
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
            | Self::State(_) => "NH-CREDENTIAL-003",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialManager, CredentialSource, resolve_or_onboard_with};
    use nan_harness_core::SecretValue;
    use nan_harness_runtime::EnvironmentSource;
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestEnvironment(BTreeMap<String, String>);

    impl EnvironmentSource for TestEnvironment {
        fn value(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
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

    #[cfg(unix)]
    #[test]
    fn private_credentials_use_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = CredentialManager::file_backend(directory.path().to_path_buf());
        manager
            .save("nan-test-key")
            .expect("credential should be saved");

        let credential_mode = std::fs::metadata(directory.path().join("nan-api-key"))
            .expect("credential should exist")
            .permissions()
            .mode()
            & 0o777;
        let receipt_mode = std::fs::metadata(directory.path().join("credential.json"))
            .expect("receipt should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(credential_mode, 0o600);
        assert_eq!(receipt_mode, 0o600);
    }
}
