use nan_harness_core::{DesktopHarnessKind, HarnessKind};
use semver::Version;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompatibilityError {
    #[error("compatibility manifest contains invalid exact-version Desktop checks: {0}")]
    InvalidDesktopChecks(&'static str),
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not build the compatibility metadata client: {0}")]
    BuildClient(reqwest::Error),
    #[error("the compatibility manifest URL is invalid: {source}")]
    InvalidUrl { source: url::ParseError },
    #[error("the compatibility manifest URL must use HTTPS")]
    InsecureUrl,
    #[error("could not fetch compatibility metadata: {0}")]
    FetchManifest(reqwest::Error),
    #[error("the compatibility server returned HTTP {0}")]
    ManifestStatus(u16),
    #[error("the compatibility manifest exceeds the 1 MiB safety limit")]
    ManifestTooLarge,
    #[error("the compatibility manifest is not valid JSON: {0}")]
    ParseManifest(serde_json::Error),
    #[error("compatibility manifest schema {0} is not supported")]
    UnsupportedManifestSchema(u8),
    #[error("compatibility manifest contains no release records")]
    EmptyReleases,
    #[error("compatibility manifest contains duplicate release {0}")]
    DuplicateRelease(Version),
    #[error("compatibility manifest contains duplicate entry for {0}")]
    DuplicateHarness(HarnessKind),
    #[error(
        "compatibility entry '{id}' has an incomplete {track} evidence pair; version and timestamp must be provided together"
    )]
    IncompleteEvidencePair { id: String, track: &'static str },
    #[error("compatibility entry '{id}' has no evidence")]
    MissingEvidence { id: String },
    #[error("compatibility entry '{id}' has an invalid {track} timestamp '{timestamp}'")]
    InvalidEvidenceTimestamp {
        id: String,
        track: &'static str,
        timestamp: String,
    },
    #[error(
        "compatibility manifest reports {harness} version {version}, below embedded minimum {minimum}"
    )]
    VersionBelowMinimum {
        harness: HarnessKind,
        version: Version,
        minimum: Version,
    },
    #[error(
        "compatibility manifest reports {harness} live version {live} newer than compatible version {compatible}"
    )]
    LiveEvidenceAhead {
        harness: HarnessKind,
        live: Version,
        compatible: Version,
    },
    #[error(
        "compatibility manifest reports {harness} live version {version}, below embedded minimum {minimum}"
    )]
    LiveVersionBelowMinimum {
        harness: HarnessKind,
        version: Version,
        minimum: Version,
    },
    #[error("embedded compatibility manifest is invalid: {0}")]
    InvalidEmbeddedManifest(String),
    #[error("the legacy compatibility feed must not carry Desktop evidence")]
    DesktopEvidenceInLegacyFeed,
    #[error("compatibility manifest contains duplicate Desktop entry for {id} on {platform}")]
    DuplicateDesktopSurface {
        id: DesktopHarnessKind,
        platform: String,
    },
    #[error("compatibility manifest reports {id} on unknown platform '{platform}'")]
    UnknownDesktopPlatform {
        id: DesktopHarnessKind,
        platform: String,
    },
    #[error(
        "compatibility manifest cannot certify {id} on {platform}, which has no supported surface"
    )]
    UnavailableDesktopSurface {
        id: DesktopHarnessKind,
        platform: String,
    },
    #[error(
        "compatibility manifest claims live verification of {id} on {platform} without {track} evidence"
    )]
    IncompleteDesktopEvidence {
        id: DesktopHarnessKind,
        platform: String,
        track: &'static str,
    },
    #[error(
        "compatibility manifest reports {id} on {platform} {track} version {version}, below embedded minimum {minimum}"
    )]
    DesktopVersionBelowMinimum {
        id: DesktopHarnessKind,
        platform: String,
        track: &'static str,
        version: Version,
        minimum: Version,
    },
    #[error("compatibility manifest has an invalid timestamp '{timestamp}' for {id} on {platform}")]
    InvalidDesktopEvidenceTimestamp {
        id: DesktopHarnessKind,
        platform: String,
        timestamp: String,
    },
    #[error("embedded desktop compatibility registry is invalid: {0}")]
    InvalidEmbeddedDesktopRegistry(String),
    #[error("could not read compatibility settings: {0}")]
    ReadState(std::io::Error),
    #[error("compatibility settings are not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("compatibility settings schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not create the nan-harness configuration directory: {0}")]
    CreateConfigDirectory(std::io::Error),
    #[error("could not serialize compatibility settings: {0}")]
    SerializeState(serde_json::Error),
    #[error("could not write compatibility settings: {0}")]
    WriteState(std::io::Error),
    #[error("the system clock is before the Unix epoch: {0}")]
    SystemClock(std::time::SystemTimeError),
}

impl CompatibilityError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingConfigDirectory
            | Self::ReadState(_)
            | Self::ParseState(_)
            | Self::UnsupportedStateSchema(_)
            | Self::CreateConfigDirectory(_)
            | Self::SerializeState(_)
            | Self::WriteState(_)
            | Self::SystemClock(_) => "NH-COMPATIBILITY-001",
            Self::BuildClient(_) | Self::FetchManifest(_) | Self::ManifestStatus(_) => {
                "NH-COMPATIBILITY-002"
            }
            Self::InvalidUrl { .. }
            | Self::InvalidDesktopChecks(_)
            | Self::InsecureUrl
            | Self::ManifestTooLarge
            | Self::ParseManifest(_)
            | Self::UnsupportedManifestSchema(_)
            | Self::EmptyReleases
            | Self::DuplicateRelease(_)
            | Self::DuplicateHarness(_)
            | Self::IncompleteEvidencePair { .. }
            | Self::MissingEvidence { .. }
            | Self::InvalidEvidenceTimestamp { .. }
            | Self::VersionBelowMinimum { .. }
            | Self::LiveVersionBelowMinimum { .. }
            | Self::LiveEvidenceAhead { .. }
            | Self::InvalidEmbeddedManifest(_)
            | Self::DesktopEvidenceInLegacyFeed
            | Self::DuplicateDesktopSurface { .. }
            | Self::UnknownDesktopPlatform { .. }
            | Self::UnavailableDesktopSurface { .. }
            | Self::IncompleteDesktopEvidence { .. }
            | Self::DesktopVersionBelowMinimum { .. }
            | Self::InvalidDesktopEvidenceTimestamp { .. }
            | Self::InvalidEmbeddedDesktopRegistry(_) => "NH-COMPATIBILITY-003",
        }
    }
}
