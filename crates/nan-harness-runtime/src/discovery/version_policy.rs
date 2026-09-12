use super::{DiscoveryError, DiscoveryOptions, DiscoveryWarning};
use nan_harness_core::{HarnessKind, VersionStatus};
use semver::Version;
use std::mem::size_of;

const FORWARD_COMPATIBILITY_QUIPS: [fn(nan_harness_i18n::Locale) -> &'static str; 10] = [
    nan_harness_i18n::messages::personality_in_nan_we_trust_text,
    nan_harness_i18n::messages::personality_may_your_compatibility_checks_be_green_and_your_stack_traces_short_text,
    nan_harness_i18n::messages::personality_say_every_prayer_you_know_text,
    nan_harness_i18n::messages::personality_pray_to_the_machine_spirits_text,
    nan_harness_i18n::messages::personality_hold_onto_your_butts_text,
    nan_harness_i18n::messages::personality_there_is_no_spoon_only_semver_text,
    nan_harness_i18n::messages::personality_here_be_dragons_forward_compatible_ones_hopefully_text,
    nan_harness_i18n::messages::personality_i_ve_got_a_good_feeling_about_this_text,
    nan_harness_i18n::messages::personality_so_long_and_thanks_for_all_the_semver_text,
    nan_harness_i18n::messages::personality_what_could_possibly_go_wrong_text,
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
) -> Vec<DiscoveryWarning> {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => Vec::new(),
        VersionStatus::NewerUntested => vec![DiscoveryWarning::NewerVersion {
            harness,
            detected: parsed_version.map_or_else(|| detected.to_owned(), ToString::to_string),
            compatible: last_compatible_version.clone(),
            quip: random_quip_index(),
        }],
        VersionStatus::OlderUnsupported => vec![DiscoveryWarning::OlderVersion {
            harness,
            detected: detected.to_owned(),
        }],
        VersionStatus::Unparseable => vec![DiscoveryWarning::UnparseableVersion {
            harness,
            detected: detected.to_owned(),
        }],
    }
}

fn random_quip_index() -> usize {
    let mut bytes = [0; size_of::<usize>()];
    if getrandom::fill(&mut bytes).is_err() {
        return 0;
    }
    usize::from_ne_bytes(bytes) % FORWARD_COMPATIBILITY_QUIPS.len()
}

pub(super) fn forward_compatibility_quip(
    index: usize,
    locale: nan_harness_i18n::Locale,
) -> &'static str {
    FORWARD_COMPATIBILITY_QUIPS[index % FORWARD_COMPATIBILITY_QUIPS.len()](locale)
}

#[cfg(test)]
fn choose_forward_compatibility_quip(random_value: usize) -> &'static str {
    forward_compatibility_quip(random_value, nan_harness_i18n::Locale::En)
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
            FORWARD_COMPATIBILITY_QUIPS[0](nan_harness_i18n::Locale::En)
        );
        assert_eq!(
            choose_forward_compatibility_quip(10),
            FORWARD_COMPATIBILITY_QUIPS[0](nan_harness_i18n::Locale::En)
        );
    }
}
