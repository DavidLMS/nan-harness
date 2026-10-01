use nan_harness_core::HarnessKind;
use nan_harness_i18n::{Locale, TerminalMessage, messages};
use semver::Version;

/// Structured advisory evidence retained separately from canonical report strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryWarning {
    NewerVersion {
        harness: HarnessKind,
        detected: String,
        compatible: Version,
        quip: usize,
    },
    OlderVersion {
        harness: HarnessKind,
        detected: String,
    },
    UnparseableVersion {
        harness: HarnessKind,
        detected: String,
    },
    ProfileProbeTimeout,
    ProfileProbeOutputLimit,
    ProfileProbeIo(String),
    ProfileUnavailable,
}

impl TerminalMessage for DiscoveryWarning {
    fn terminal_message(&self, locale: Locale) -> String {
        match self {
            Self::NewerVersion {
                harness,
                detected,
                compatible,
                quip,
            } => messages::discovery_newer(
                locale,
                compatible,
                detected,
                harness,
                super::version_policy::forward_compatibility_quip(*quip, locale),
            ),
            Self::OlderVersion { harness, detected } => {
                messages::discovery_older(locale, detected, harness)
            }
            Self::UnparseableVersion { harness, detected } => {
                messages::discovery_unparseable(locale, detected, harness)
            }
            Self::ProfileProbeTimeout => {
                messages::discovery_profile_failure(locale, &messages::error_probe_timeout(locale))
            }
            Self::ProfileProbeOutputLimit => messages::discovery_profile_failure(
                locale,
                &messages::error_probe_output_limit(locale),
            ),
            Self::ProfileProbeIo(source) => messages::discovery_profile_failure(
                locale,
                &messages::error_probe_io(locale, source),
            ),
            Self::ProfileUnavailable => messages::discovery_profile_unavailable(locale),
        }
    }
}
