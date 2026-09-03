use semver::Version;
use serde::{Deserialize, Serialize};

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
