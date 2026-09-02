use super::CredentialError;
use super::receipts::open_private_file_for_read;
use crate::commands::persistence::{config_directory, discover_models, write_private_file};
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::ResolvedConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERIFICATION_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const VERIFICATION_CACHE_TTL: Duration = Duration::from_hours(1);
const VERIFICATION_CACHE_FILE_NAME: &str = "credential-verification.json";
pub(super) const VERIFICATION_CACHE_SCHEMA_VERSION: u8 = 1;
pub(super) const VERIFICATION_RECEIPT_REPAIR_WARNING: &str =
    "warning: restored private permissions on the NaN verification receipt.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationReceipt {
    pub(super) schema_version: u8,
    pub(super) provider_base_url: String,
    pub(super) credential_fingerprint: String,
    pub(super) verified_at_unix_seconds: u64,
}

pub(crate) async fn verify(config: &ResolvedConfig) -> Result<(), CredentialError> {
    verify_models(config).await.map(|_| ())
}

pub(super) async fn verify_models(
    config: &ResolvedConfig,
) -> Result<Vec<CodingModelProfile>, CredentialError> {
    match tokio::time::timeout(VERIFICATION_TIMEOUT, discover_models(config)).await {
        Ok(Ok(models)) => Ok(models),
        Ok(Err(error)) => Err(CredentialError::Verification(error)),
        Err(_) => Err(CredentialError::VerificationTimeout),
    }
}

pub(super) async fn verify_cached(
    config: &ResolvedConfig,
) -> Result<Option<Vec<CodingModelProfile>>, CredentialError> {
    let fingerprint = credential_fingerprint(config)?;
    let cache_path = verification_cache_path()?;
    verify_cached_at(config, &cache_path, &fingerprint).await
}

pub(super) async fn verify_cached_at(
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

pub(crate) fn credential_fingerprint(config: &ResolvedConfig) -> Result<String, CredentialError> {
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

pub(super) fn verification_cache_is_current(
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

pub(super) fn unix_time() -> Result<u64, CredentialError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(CredentialError::SystemTime)
}

pub(super) fn is_rejected(error: &CredentialError) -> bool {
    matches!(
        error,
        CredentialError::Verification(
            crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401 | 403)
        )
    )
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
