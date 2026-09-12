use nan_harness_core::{DesktopHarnessKind, HarnessKind};
use semver::Version;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompatibilityError {
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

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for CompatibilityError {
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive terminal projection of compatibility errors"
    )]
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::MissingConfigDirectory => m::error_compatibility_missing_config_directory(locale),
            Self::BuildClient(field_0) => m::error_compatibility_build_client(locale, &(field_0)),
            Self::InvalidUrl { source } => m::error_compatibility_invalid_url(locale, &(source)),
            Self::InsecureUrl => m::error_compatibility_insecure_url(locale),
            Self::FetchManifest(field_0) => {
                m::error_compatibility_fetch_manifest(locale, &(field_0))
            }
            Self::ManifestStatus(field_0) => {
                m::error_compatibility_manifest_status(locale, &(field_0))
            }
            Self::ManifestTooLarge => m::error_compatibility_manifest_too_large(locale),
            Self::ParseManifest(field_0) => {
                m::error_compatibility_parse_manifest(locale, &(field_0))
            }
            Self::UnsupportedManifestSchema(field_0) => {
                m::error_compatibility_unsupported_manifest_schema(locale, &(field_0))
            }
            Self::EmptyReleases => m::error_compatibility_empty_releases(locale),
            Self::DuplicateRelease(field_0) => {
                m::error_compatibility_duplicate_release(locale, &(field_0))
            }
            Self::DuplicateHarness(field_0) => {
                m::error_compatibility_duplicate_harness(locale, &(field_0))
            }
            Self::IncompleteEvidencePair { id, track } => {
                m::error_compatibility_incomplete_evidence_pair(locale, &(id), &(track))
            }
            Self::MissingEvidence { id } => m::error_compatibility_missing_evidence(locale, &(id)),
            Self::InvalidEvidenceTimestamp {
                id,
                track,
                timestamp,
            } => m::error_compatibility_invalid_evidence_timestamp(
                locale,
                &(id),
                &(timestamp),
                &(track),
            ),
            Self::VersionBelowMinimum {
                harness,
                version,
                minimum,
            } => m::error_compatibility_version_below_minimum(
                locale,
                &(harness),
                &(minimum),
                &(version),
            ),
            Self::LiveEvidenceAhead {
                harness,
                live,
                compatible,
            } => m::error_compatibility_live_evidence_ahead(
                locale,
                &(compatible),
                &(harness),
                &(live),
            ),
            Self::LiveVersionBelowMinimum {
                harness,
                version,
                minimum,
            } => m::error_compatibility_live_version_below_minimum(
                locale,
                &(harness),
                &(minimum),
                &(version),
            ),
            Self::InvalidEmbeddedManifest(field_0) => {
                m::error_compatibility_invalid_embedded_manifest(locale, &(field_0))
            }
            Self::DesktopEvidenceInLegacyFeed => {
                m::error_compatibility_desktop_evidence_in_legacy_feed(locale)
            }
            Self::DuplicateDesktopSurface { id, platform } => {
                m::error_compatibility_duplicate_desktop_surface(locale, &(id), &(platform))
            }
            Self::UnknownDesktopPlatform { id, platform } => {
                m::error_compatibility_unknown_desktop_platform(locale, &(id), &(platform))
            }
            Self::UnavailableDesktopSurface { id, platform } => {
                m::error_compatibility_unavailable_desktop_surface(locale, &(id), &(platform))
            }
            Self::IncompleteDesktopEvidence {
                id,
                platform,
                track,
            } => m::error_compatibility_incomplete_desktop_evidence(
                locale,
                &(id),
                &(platform),
                &(track),
            ),
            Self::DesktopVersionBelowMinimum {
                id,
                platform,
                track,
                version,
                minimum,
            } => m::error_compatibility_desktop_version_below_minimum(
                locale,
                &(id),
                &(minimum),
                &(platform),
                &(track),
                &(version),
            ),
            Self::InvalidDesktopEvidenceTimestamp {
                id,
                platform,
                timestamp,
            } => m::error_compatibility_invalid_desktop_evidence_timestamp(
                locale,
                &(id),
                &(platform),
                &(timestamp),
            ),
            Self::InvalidEmbeddedDesktopRegistry(field_0) => {
                m::error_compatibility_invalid_embedded_desktop_registry(locale, &(field_0))
            }
            Self::ReadState(field_0) => m::error_compatibility_read_state(locale, &(field_0)),
            Self::ParseState(field_0) => m::error_compatibility_parse_state(locale, &(field_0)),
            Self::UnsupportedStateSchema(field_0) => {
                m::error_compatibility_unsupported_state_schema(locale, &(field_0))
            }
            Self::CreateConfigDirectory(field_0) => {
                m::error_compatibility_create_config_directory(locale, &(field_0))
            }
            Self::SerializeState(field_0) => {
                m::error_compatibility_serialize_state(locale, &(field_0))
            }
            Self::WriteState(field_0) => m::error_compatibility_write_state(locale, &(field_0)),
            Self::SystemClock(field_0) => m::error_compatibility_system_clock(locale, &(field_0)),
        }
    }
}
