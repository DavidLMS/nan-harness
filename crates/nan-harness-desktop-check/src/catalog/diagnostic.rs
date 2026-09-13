//! Closed, bounded diagnostics for credential-free preparation failures.

use crate::report::Reason;
use nan_harness_core::DesktopHarnessKind;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "DESKTOP_PREPARE_DIAGNOSTIC:";
const MAX_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Stage {
    RootEnumeration,
    CandidateMetadata,
    CandidateCanonicalization,
    CandidateRead,
    Architecture,
    VersionResource,
    FrozenResolution,
    Installation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ErrorCategory {
    Unsupported,
    Ambiguous,
    Incomplete,
    Unreadable,
    VersionUnknown,
    ResolutionFailed,
    InstallationFailed,
    UpstreamUnsupported,
    UnqualifiedPlatform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Event {
    pub(crate) schema_version: u8,
    pub(crate) app: DesktopHarnessKind,
    pub(crate) stage: Stage,
    pub(crate) error_category: ErrorCategory,
    pub(crate) reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) os_error: Option<i32>,
}

pub(crate) fn emit(event: Event) {
    let Ok(bytes) = serde_json::to_vec(&event) else {
        return;
    };
    if bytes.len() <= MAX_BYTES
        && let Ok(line) = String::from_utf8(bytes)
    {
        eprintln!("{PREFIX}{line}");
    }
}

pub(crate) fn category(error: crate::catalog::DiscoveryError) -> ErrorCategory {
    match error {
        crate::catalog::DiscoveryError::Ambiguous => ErrorCategory::Ambiguous,
        crate::catalog::DiscoveryError::Unsupported => ErrorCategory::Unsupported,
        crate::catalog::DiscoveryError::RootEnumeration
        | crate::catalog::DiscoveryError::CandidateMetadata
        | crate::catalog::DiscoveryError::CandidateCanonicalization
        | crate::catalog::DiscoveryError::CandidateRead
        | crate::catalog::DiscoveryError::VersionResource
        | crate::catalog::DiscoveryError::Architecture
        | crate::catalog::DiscoveryError::Unreadable => ErrorCategory::Unreadable,
        crate::catalog::DiscoveryError::Incomplete => ErrorCategory::Incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_diagnostic_schema_is_closed_and_typed() {
        let event = Event {
            schema_version: 1,
            app: DesktopHarnessKind::Claude,
            stage: Stage::RootEnumeration,
            error_category: ErrorCategory::Unreadable,
            reason: Reason::InstallationUnreadable,
            os_error: Some(5),
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["app"], "claude-desktop");
        assert_eq!(value["stage"], "root-enumeration");
        assert!(serde_json::from_value::<Event>(value.clone()).is_ok());
        assert!(
            serde_json::from_value::<Event>(serde_json::json!({
                "schemaVersion": 1,
                "app": "claude-desktop",
                "stage": "not-a-stage",
                "errorCategory": "unreadable",
                "reason": "installation-unreadable"
            }))
            .is_err()
        );
        let mut extra = value.as_object().unwrap().clone();
        extra.insert("path".into(), "must-not-escape".into());
        assert!(serde_json::from_value::<Event>(extra.into()).is_err());
    }
}

pub(crate) fn stage(error: crate::catalog::DiscoveryError) -> Stage {
    match error {
        crate::catalog::DiscoveryError::RootEnumeration => Stage::RootEnumeration,
        crate::catalog::DiscoveryError::CandidateMetadata
        | crate::catalog::DiscoveryError::Ambiguous
        | crate::catalog::DiscoveryError::Unsupported
        | crate::catalog::DiscoveryError::Unreadable => Stage::CandidateMetadata,
        crate::catalog::DiscoveryError::CandidateCanonicalization => {
            Stage::CandidateCanonicalization
        }
        crate::catalog::DiscoveryError::CandidateRead
        | crate::catalog::DiscoveryError::Incomplete => Stage::CandidateRead,
        crate::catalog::DiscoveryError::VersionResource => Stage::VersionResource,
        crate::catalog::DiscoveryError::Architecture => Stage::Architecture,
    }
}
