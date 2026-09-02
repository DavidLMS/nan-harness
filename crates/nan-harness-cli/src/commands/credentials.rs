mod error;
mod receipts;
mod storage;
mod verification;
mod workflow;

pub(crate) use error::CredentialError;
pub(crate) use storage::{CredentialManager, CredentialSource};
pub(crate) use verification::{credential_fingerprint, verify};
pub(crate) use workflow::{
    ResolvedLaunchConfig, resolve_existing_config, resolve_or_onboard, resolve_saved_or_onboard,
    run, saved_credential_fingerprint,
};

#[cfg(test)]
use receipts::{
    CREDENTIAL_METADATA_REPAIR_WARNING, SAVED_KEY_REPAIR_WARNING, private_file_repair_warning,
};
#[cfg(test)]
use verification::{
    VERIFICATION_CACHE_SCHEMA_VERSION, VERIFICATION_CACHE_TTL, VERIFICATION_RECEIPT_REPAIR_WARNING,
    VerificationReceipt, is_rejected, unix_time, verification_cache_is_current, verify_cached_at,
};
#[cfg(test)]
use workflow::{
    render_first_harness_hint, render_missing_credential_hint, resolve_or_onboard_with,
};

#[cfg(test)]
mod tests;
