use crate::desktop_compatibility::DesktopCompatibilityEvidence;
use semver::Version;
use serde::{Deserialize, Serialize};

/// Schema version of the legacy, CLI-only compatibility feed.
///
/// Clients built before the unified feed parse this asset with `deny_unknown_fields`, so it must
/// never gain a field. The unified asset is published separately as schema v3.
pub const LEGACY_FEED_SCHEMA_VERSION: u8 = 2;
/// Schema version of the unified feed carrying CLI and Desktop evidence.
pub const UNIFIED_FEED_SCHEMA_VERSION: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationManifest {
    pub schema_version: u8,
    pub releases: Vec<VerificationRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationRelease {
    pub nan_harness_version: Version,
    #[serde(alias = "harnesses")]
    pub verifications: Vec<VerificationEntry>,
    /// Desktop evidence, present only in the unified schema-v3 feed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop_verifications: Vec<DesktopVerificationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationEntry {
    pub id: String,
    #[serde(default)]
    pub last_compatible_version: Option<Version>,
    #[serde(default)]
    pub compatible_at: Option<String>,
    #[serde(default)]
    pub last_live_verified_version: Option<Version>,
    #[serde(default)]
    pub live_verified_at: Option<String>,
}

/// Desktop evidence for one surface on one platform, in one nan-harness release.
///
/// The application and its bundled runtime are certified together: a record that claims live
/// verification must carry every version bound the embedded registry certifies for that surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopVerificationEntry {
    pub id: String,
    pub platform: String,
    pub evidence: DesktopCompatibilityEvidence,
    #[serde(default)]
    pub last_compatible_app_version: Option<Version>,
    #[serde(default)]
    pub last_compatible_runtime_version: Option<Version>,
    pub compatible_at: String,
}
