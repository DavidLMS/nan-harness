use super::CredentialError;
use super::receipts::{
    CREDENTIAL_RECEIPT_SCHEMA_VERSION, CredentialReceipt, SAVED_KEY_REPAIR_WARNING, StoredBackend,
    open_private_file_for_read, read_credential_receipt, remove_file_if_present,
    write_credential_receipt,
};
use crate::commands::persistence::{config_directory, write_private_file};
use keyring::{Entry, Error as KeyringError};
use nan_harness_core::SecretValue;
use std::env;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;

const CREDENTIAL_BACKEND_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CREDENTIAL_BACKEND";
const CREDENTIAL_FILE_NAME: &str = "nan-api-key";
const CREDENTIAL_RECEIPT_FILE_NAME: &str = "credential.json";
const KEYRING_SERVICE: &str = "nan-harness";
const KEYRING_USER: &str = "nan-api-key";

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
        read_credential_receipt(&self.receipt_path)
    }

    fn save_receipt(&self, receipt: CredentialReceipt) -> Result<(), CredentialError> {
        write_credential_receipt(&self.receipt_path, receipt)
    }

    #[cfg(test)]
    pub(super) fn file_backend(directory: impl Into<PathBuf>) -> Self {
        Self::with_preference(directory, BackendPreference::File)
            .expect("test credential directory should be valid")
    }
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
