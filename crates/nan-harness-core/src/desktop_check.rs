//! Exact-version Desktop evidence shared by the checker, publisher and clients.

use crate::DesktopHarnessKind;
use semver::Version;
use serde::{Deserialize, Serialize};

/// Successful checks for one indivisible application/runtime/platform/architecture tuple.
/// Absence of a timestamp means no evidence for that track, never a failed check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCheck {
    pub id: DesktopHarnessKind,
    pub platform: String,
    pub architecture: String,
    pub app_version: Version,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<Version>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_verified_at: Option<String>,
}

impl DesktopCheck {
    /// Evidence tracks may be merged only when every tested dimension is identical.
    #[must_use]
    pub fn same_target(&self, other: &Self) -> bool {
        self.id == other.id
            && self.platform == other.platform
            && self.architecture == other.architecture
            && self.app_version == other.app_version
            && self.runtime_version == other.runtime_version
    }
}
