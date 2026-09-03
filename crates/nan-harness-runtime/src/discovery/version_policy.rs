use super::{DiscoveryError, DiscoveryOptions};
use nan_harness_core::{HarnessKind, VersionStatus};
use semver::Version;
use std::mem::size_of;

const FORWARD_COMPATIBILITY_QUIPS: [&str; 10] = [
    "In NaN we trust!",
    "May your compatibility checks be green and your stack traces short.",
    "Say every prayer you know.",
    "Pray to the machine spirits.",
    "Hold onto your butts.",
    "There is no spoon, only semver.",
    "Here be dragons—forward-compatible ones, hopefully.",
    "I've got a good feeling about this.",
    "So long, and thanks for all the semver.",
    "What could possibly go wrong?",
];

pub(super) fn parse_version(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .rsplit_once('/')
            .map_or(token, |(_, version)| version)
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        Version::parse(candidate).ok()
    })
}

pub(super) fn enforce(
    harness: HarnessKind,
    status: VersionStatus,
    detected: &str,
    options: DiscoveryOptions,
) -> Result<(), DiscoveryError> {
    match status {
        VersionStatus::OlderUnsupported if !options.allow_unsupported => {
            Err(DiscoveryError::UnsupportedVersion {
                harness,
                detected: detected.to_owned(),
            })
        }
        VersionStatus::Unparseable if !options.allow_untested => {
            Err(DiscoveryError::UnparseableVersion {
                harness,
                detected: detected.to_owned(),
            })
        }
        VersionStatus::Tested
        | VersionStatus::Supported
        | VersionStatus::NewerUntested
        | VersionStatus::OlderUnsupported
        | VersionStatus::Unparseable => Ok(()),
    }
}

pub(super) fn warnings(
    harness: HarnessKind,
    status: VersionStatus,
    detected: &str,
    parsed_version: Option<&Version>,
    last_compatible_version: &Version,
) -> Vec<String> {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => Vec::new(),
        VersionStatus::NewerUntested => {
            let detected_version =
                parsed_version.map_or_else(|| detected.to_owned(), ToString::to_string);
            vec![format!(
                "The detected {harness} ({detected_version}) is newer than the last version confirmed compatible with this nan-harness release ({last_compatible_version}); continuing with forward-compatible safeguards. {}",
                random_forward_compatibility_quip()
            )]
        }
        VersionStatus::OlderUnsupported => vec![format!(
            "{harness} version '{detected}' is older than the supported minimum."
        )],
        VersionStatus::Unparseable => vec![format!(
            "nan-harness could not parse the {harness} version from '{detected}'."
        )],
    }
}

fn random_forward_compatibility_quip() -> &'static str {
    let mut bytes = [0; size_of::<usize>()];
    if getrandom::fill(&mut bytes).is_err() {
        return FORWARD_COMPATIBILITY_QUIPS[0];
    }
    choose_forward_compatibility_quip(usize::from_ne_bytes(bytes))
}

fn choose_forward_compatibility_quip(random_value: usize) -> &'static str {
    FORWARD_COMPATIBILITY_QUIPS[random_value % FORWARD_COMPATIBILITY_QUIPS.len()]
}

#[cfg(test)]
mod tests {
    use super::{
        FORWARD_COMPATIBILITY_QUIPS, choose_forward_compatibility_quip, enforce, parse_version,
    };
    use crate::discovery::{DiscoveryError, DiscoveryOptions};
    use nan_harness_core::{HarnessKind, VersionStatus};
    use semver::Version;

    #[test]
    fn version_parser_preserves_supported_output_shapes() {
        assert_eq!(
            parse_version("claude v2.1.243"),
            Some(Version::new(2, 1, 243))
        );
        assert_eq!(parse_version("omp/18.0.11"), Some(Version::new(18, 0, 11)));
        assert_eq!(parse_version("development build"), None);
    }

    #[test]
    fn policy_requires_only_its_matching_override() {
        assert!(matches!(
            enforce(
                HarnessKind::ClaudeCode,
                VersionStatus::OlderUnsupported,
                "claude 2.0.0",
                DiscoveryOptions::default()
            ),
            Err(DiscoveryError::UnsupportedVersion { .. })
        ));
        assert!(matches!(
            enforce(
                HarnessKind::ClaudeCode,
                VersionStatus::Unparseable,
                "development build",
                DiscoveryOptions {
                    allow_unsupported: true,
                    allow_untested: false,
                }
            ),
            Err(DiscoveryError::UnparseableVersion { .. })
        ));
        assert!(
            enforce(
                HarnessKind::ClaudeCode,
                VersionStatus::OlderUnsupported,
                "claude 2.0.0",
                DiscoveryOptions {
                    allow_unsupported: true,
                    allow_untested: false,
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn forward_compatibility_quips_have_the_requested_variety() {
        assert_eq!(FORWARD_COMPATIBILITY_QUIPS.len(), 10);
        assert_eq!(
            choose_forward_compatibility_quip(0),
            FORWARD_COMPATIBILITY_QUIPS[0]
        );
        assert_eq!(
            choose_forward_compatibility_quip(10),
            FORWARD_COMPATIBILITY_QUIPS[0]
        );
    }
}
