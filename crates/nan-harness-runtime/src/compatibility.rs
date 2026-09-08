mod desktop;
mod desktop_checks;
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
pub use manifest::{
    DesktopVerificationEntry, LEGACY_FEED_SCHEMA_VERSION, UNIFIED_FEED_SCHEMA_VERSION,
    VERSIONED_FEED_SCHEMA_VERSION, VerificationEntry, VerificationManifest, VerificationRelease,
};
pub use refresh::{RefreshOutcome, refresh_compatibility_manifest};
pub(crate) use refresh::{apply_cached_desktop_verifications, apply_cached_verifications};

#[cfg(test)]
mod tests;
