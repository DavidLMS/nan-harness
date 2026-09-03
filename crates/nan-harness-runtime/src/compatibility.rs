mod environment;
mod error;
mod evidence;
mod manifest;
mod network;
mod refresh;
mod state;
mod validation;

pub use environment::{
    COMPATIBILITY_MANIFEST_ENVIRONMENT_VARIABLE,
    DISABLE_COMPATIBILITY_REFRESH_ENVIRONMENT_VARIABLE, automatic_refresh_enabled,
    compatibility_manifest_url,
};
pub use error::CompatibilityError;
pub use manifest::{VerificationEntry, VerificationManifest, VerificationRelease};
pub(crate) use refresh::apply_cached_verifications;
pub use refresh::{RefreshOutcome, refresh_compatibility_manifest};

#[cfg(test)]
mod tests;
