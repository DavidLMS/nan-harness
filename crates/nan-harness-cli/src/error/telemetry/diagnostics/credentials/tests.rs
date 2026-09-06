use super::classify;
use crate::commands::credentials::CredentialError;
use crate::commands::persistence::PersistenceError;
use keyring::Error as KeyringError;
use nan_harness_core::SecretError;
use nan_harness_runtime::ConfigError;
use nan_harness_telemetry::event::FailureCause;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, SystemTimeError};

const FAKE_PATH: &str = "/private/credentials-fixture/fake-secret/config.json";
const FAKE_TOKEN: &str = "fixture-token-must-not-appear";
const STATE_HTTP_STATUS: u16 = 418;
const VERIFICATION_HTTP_STATUS: u16 = 503;

fn assert_classification(error: &CredentialError, expected: (FailureCause, Option<u16>)) {
    assert_eq!(classify(error), expected);
}

fn io_error(kind: io::ErrorKind) -> io::Error {
    io::Error::new(kind, FAKE_TOKEN)
}

fn sensitive_keyring_denial() -> KeyringError {
    KeyringError::PlatformFailure(Box::new(io::Error::other(format!(
        "{FAKE_TOKEN}: {FAKE_PATH}"
    ))))
}

fn sensitive_serialization_error() -> serde_json::Error {
    struct Fixture;

    impl Serialize for Fixture {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(<S::Error as serde::ser::Error>::custom(FAKE_TOKEN))
        }
    }

    serde_json::to_string(&Fixture).expect_err("fixture must fail to serialize")
}

fn sensitive_parse_error() -> serde_json::Error {
    #[derive(Debug)]
    struct Fixture;

    impl<'de> Deserialize<'de> for Fixture {
        fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Err(<D::Error as serde::de::Error>::custom(FAKE_TOKEN))
        }
    }

    serde_json::from_value::<Fixture>(serde_json::Value::Null)
        .expect_err("fixture must fail to deserialize")
}

fn fake_path() -> PathBuf {
    PathBuf::from(FAKE_PATH)
}

fn system_time_error() -> SystemTimeError {
    SystemTime::UNIX_EPOCH
        .duration_since(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
        .expect_err("the Unix epoch must be earlier than one second later")
}

#[test]
fn missing_credentials_and_configuration_errors_map_to_typed_causes() {
    let cases = [
        (
            CredentialError::MissingCredential,
            (FailureCause::MissingCredential, None),
        ),
        (
            CredentialError::MissingSavedCredential,
            (FailureCause::MissingCredential, None),
        ),
        (
            CredentialError::InteractiveLoginRequired,
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::InvalidConfigDirectory(fake_path()),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::InvalidBackend(FAKE_TOKEN.to_owned()),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::NonUnicodeBackend,
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::LogoutConfirmationRequired,
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::LogoutModeRequired,
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::InvalidLogoutChoice,
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::ConfigurationOperation(format!("{FAKE_TOKEN}: {FAKE_PATH}")),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::Secret(SecretError::InvalidReference(FAKE_TOKEN.to_owned())),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::Config(ConfigError::MissingApiKey),
            (FailureCause::InvalidConfiguration, None),
        ),
    ];

    for (error, expected) in &cases {
        assert_classification(error, *expected);
    }
}

#[test]
fn receipt_and_secret_input_errors_map_to_invalid_configuration() {
    let cases = [
        (
            CredentialError::ParseReceipt(sensitive_parse_error()),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::UnsupportedReceiptSchema(9),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::SerializeReceipt(sensitive_serialization_error()),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::ParseVerificationReceipt(sensitive_parse_error()),
            (FailureCause::InvalidConfiguration, None),
        ),
        (
            CredentialError::SerializeVerificationReceipt(sensitive_serialization_error()),
            (FailureCause::InvalidConfiguration, None),
        ),
    ];

    for (error, expected) in cases {
        assert_classification(&error, expected);
    }
}

#[test]
fn prompt_and_file_io_errors_preserve_io_kinds() {
    let cases = [
        (
            CredentialError::Prompt(io_error(io::ErrorKind::NotFound)),
            (FailureCause::NotFound, None),
        ),
        (
            CredentialError::Prompt(io_error(io::ErrorKind::PermissionDenied)),
            (FailureCause::PermissionDenied, None),
        ),
        (
            CredentialError::Prompt(io_error(io::ErrorKind::TimedOut)),
            (FailureCause::Timeout, None),
        ),
        (
            CredentialError::Prompt(io_error(io::ErrorKind::ConnectionRefused)),
            (FailureCause::Network, None),
        ),
        (
            CredentialError::Prompt(io_error(io::ErrorKind::InvalidData)),
            (FailureCause::Filesystem, None),
        ),
        (
            CredentialError::ReadFile {
                path: fake_path(),
                source: io_error(io::ErrorKind::PermissionDenied),
            },
            (FailureCause::PermissionDenied, None),
        ),
        (
            CredentialError::RemoveFile {
                path: fake_path(),
                source: io_error(io::ErrorKind::NotFound),
            },
            (FailureCause::NotFound, None),
        ),
    ];

    for (error, expected) in &cases {
        assert_classification(error, *expected);
    }
}

#[test]
fn verification_and_state_delegations_preserve_http_statuses() {
    let cases = [
        (
            CredentialError::Verification(PersistenceError::ModelDiscoveryStatus(
                VERIFICATION_HTTP_STATUS,
            )),
            (FailureCause::HttpStatus, Some(VERIFICATION_HTTP_STATUS)),
        ),
        (
            CredentialError::State(PersistenceError::ModelDiscoveryStatus(STATE_HTTP_STATUS)),
            (FailureCause::HttpStatus, Some(STATE_HTTP_STATUS)),
        ),
        (
            CredentialError::Verification(PersistenceError::ReadFile {
                path: fake_path(),
                source: io_error(io::ErrorKind::PermissionDenied),
            }),
            (FailureCause::PermissionDenied, None),
        ),
        (
            CredentialError::State(PersistenceError::WriteFile {
                path: fake_path(),
                source: io_error(io::ErrorKind::TimedOut),
            }),
            (FailureCause::Timeout, None),
        ),
    ];

    for (error, expected) in &cases {
        assert_classification(error, *expected);
    }
}

#[test]
fn credential_platform_and_time_errors_map_to_typed_causes() {
    let cases = [
        (
            CredentialError::VerificationTimeout,
            (FailureCause::Timeout, None),
        ),
        (
            CredentialError::Keyring(sensitive_keyring_denial()),
            (FailureCause::PermissionDenied, None),
        ),
        (
            CredentialError::SystemTime(system_time_error()),
            (FailureCause::Internal, None),
        ),
        (
            CredentialError::MissingConfigDirectory,
            (FailureCause::Filesystem, None),
        ),
    ];

    for (error, expected) in &cases {
        assert_classification(error, *expected);
    }
}
