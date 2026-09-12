use super::super::discovery::HarnessDiscovery;
use super::super::models::{
    DOCTOR_SCHEMA_VERSION, DiagnosticLevel, HarnessDetails, HarnessDoctorReport, HarnessReport,
    HarnessTextReport, HarnessTextStatus,
};
use nan_harness_core::{HarnessKind, VersionStatus};
use nan_harness_runtime::{DiscoveryError, DiscoveryReport};

pub(crate) fn harness_json_report(
    harness: HarnessKind,
    discovery: Result<DiscoveryReport, DiscoveryError>,
) -> HarnessDoctorReport {
    match discovery {
        Ok(discovery) => HarnessDoctorReport {
            schema_version: DOCTOR_SCHEMA_VERSION,
            offline: false,
            harness: discovery.harness.kind,
            level: diagnostic_level(discovery.harness.version_status),
            installed: true,
            version: normalized_version(&discovery.harness.detected_version),
            minimum_supported_version: Some(discovery.minimum_supported_version.to_string()),
            last_compatible_version: Some(discovery.last_compatible_version.to_string()),
            compatible_at: Some(discovery.compatible_at),
            last_live_verified_version: discovery
                .last_live_verified_version
                .map(|version| version.to_string()),
            live_verified_at: discovery.live_verified_at,
            compatibility: Some(compatibility_label(discovery.harness.version_status)),
            warnings: discovery.warnings,
            error_code: None,
            safe_to_share: true,
        },
        Err(error) => HarnessDoctorReport {
            schema_version: DOCTOR_SCHEMA_VERSION,
            offline: false,
            harness,
            level: DiagnosticLevel::Error,
            installed: !matches!(&error, DiscoveryError::ExecutableNotFound(_)),
            version: None,
            minimum_supported_version: None,
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
            compatibility: None,
            warnings: Vec::new(),
            error_code: Some(error.code()),
            safe_to_share: true,
        },
    }
}

pub(super) fn harness_json_reports(discoveries: Vec<HarnessDiscovery>) -> Vec<HarnessReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| harness_report(harness, discovery))
        .collect()
}

fn harness_report(
    harness: HarnessKind,
    discovery: Result<DiscoveryReport, DiscoveryError>,
) -> HarnessReport {
    match discovery {
        Ok(discovery) => HarnessReport {
            id: harness,
            level: diagnostic_level(discovery.harness.version_status),
            installed: true,
            version: normalized_version(&discovery.harness.detected_version),
            minimum_supported_version: Some(discovery.minimum_supported_version.to_string()),
            last_compatible_version: Some(discovery.last_compatible_version.to_string()),
            compatible_at: Some(discovery.compatible_at),
            last_live_verified_version: discovery
                .last_live_verified_version
                .map(|version| version.to_string()),
            live_verified_at: discovery.live_verified_at,
            compatibility: Some(compatibility_label(discovery.harness.version_status)),
            error_code: None,
        },
        Err(DiscoveryError::ExecutableNotFound(_)) => HarnessReport {
            id: harness,
            level: DiagnosticLevel::Info,
            installed: false,
            version: None,
            minimum_supported_version: None,
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
            compatibility: None,
            error_code: None,
        },
        Err(error) => HarnessReport {
            id: harness,
            level: DiagnosticLevel::Error,
            installed: true,
            version: None,
            minimum_supported_version: None,
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
            compatibility: None,
            error_code: Some(error.code()),
        },
    }
}

pub(super) fn harness_text_reports(discoveries: Vec<HarnessDiscovery>) -> Vec<HarnessTextReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| {
            let status = match discovery {
                Ok(discovery) => {
                    let version = normalized_version(&discovery.harness.detected_version)
                        .unwrap_or_else(|| {
                            nan_harness_i18n::messages::doctor_unparseable_text(
                                nan_harness_i18n::locale(),
                            )
                            .to_owned()
                        });
                    let (level, label) = match discovery.harness.version_status {
                        VersionStatus::Tested => (
                            "OK",
                            nan_harness_i18n::messages::doctor_tested_text(
                                nan_harness_i18n::locale(),
                            ),
                        ),
                        VersionStatus::Supported => (
                            "OK",
                            nan_harness_i18n::messages::doctor_supported_text(
                                nan_harness_i18n::locale(),
                            ),
                        ),
                        VersionStatus::NewerUntested => (
                            "WARN",
                            nan_harness_i18n::messages::doctor_newer_text(
                                nan_harness_i18n::locale(),
                            ),
                        ),
                        VersionStatus::OlderUnsupported => (
                            "ERROR",
                            nan_harness_i18n::messages::doctor_unsupported_text(
                                nan_harness_i18n::locale(),
                            ),
                        ),
                        VersionStatus::Unparseable => (
                            "WARN",
                            nan_harness_i18n::messages::doctor_version_unparseable_text(
                                nan_harness_i18n::locale(),
                            ),
                        ),
                    };
                    HarnessTextStatus::Installed {
                        version,
                        level,
                        label,
                    }
                }
                Err(DiscoveryError::ExecutableNotFound(_)) => HarnessTextStatus::NotInstalled,
                Err(error) => HarnessTextStatus::Failed(error.code()),
            };
            HarnessTextReport { harness, status }
        })
        .collect()
}

pub(crate) fn harness_details(discovery: DiscoveryReport) -> HarnessDetails {
    let warnings = discovery.terminal_warnings(nan_harness_i18n::locale());
    HarnessDetails {
        harness: discovery.harness.kind,
        executable: discovery.harness.executable,
        detected_version: discovery.harness.detected_version,
        minimum_supported_version: discovery.minimum_supported_version.to_string(),
        last_compatible_version: discovery.last_compatible_version.to_string(),
        compatible_at: discovery.compatible_at,
        last_live_verified_version: discovery
            .last_live_verified_version
            .map(|version| version.to_string()),
        live_verified_at: discovery.live_verified_at,
        compatibility: terminal_compatibility_label(discovery.harness.version_status),
        warnings,
    }
}

fn diagnostic_level(status: VersionStatus) -> DiagnosticLevel {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => DiagnosticLevel::Ok,
        VersionStatus::NewerUntested | VersionStatus::Unparseable => DiagnosticLevel::Warning,
        VersionStatus::OlderUnsupported => DiagnosticLevel::Error,
    }
}

fn normalized_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .rsplit_once('/')
            .map_or(token, |(_, version)| version)
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}

const fn compatibility_label(status: VersionStatus) -> &'static str {
    match status {
        VersionStatus::Tested => "tested",
        VersionStatus::Supported => "supported",
        VersionStatus::NewerUntested => "newer-untested",
        VersionStatus::OlderUnsupported => "older-unsupported",
        VersionStatus::Unparseable => "unparseable",
    }
}

fn terminal_compatibility_label(status: VersionStatus) -> &'static str {
    use nan_harness_i18n::{locale, messages};
    match status {
        VersionStatus::Tested => messages::doctor_tested_text(locale()),
        VersionStatus::Supported => messages::doctor_supported_text(locale()),
        VersionStatus::NewerUntested => messages::doctor_newer_untested_text(locale()),
        VersionStatus::OlderUnsupported => messages::doctor_older_unsupported_text(locale()),
        VersionStatus::Unparseable => messages::doctor_unparseable_text(locale()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_version_accepts_slash_prefixed_versions() {
        assert_eq!(
            normalized_version("omp/18.0.11"),
            Some("18.0.11".to_owned())
        );
    }

    #[test]
    fn harness_reports_preserve_all_harnesses_and_schema() {
        let discoveries = HarnessKind::ALL
            .into_iter()
            .map(|harness| {
                (
                    harness,
                    Err(DiscoveryError::ExecutableNotFound(
                        harness.binary_name().to_owned(),
                    )),
                )
            })
            .collect();

        let reports = harness_json_reports(discoveries);

        assert_eq!(DOCTOR_SCHEMA_VERSION, 8);
        assert_eq!(reports.len(), HarnessKind::ALL.len());
        assert_eq!(
            reports.iter().map(|report| report.id).collect::<Vec<_>>(),
            HarnessKind::ALL,
        );
        assert!(reports.iter().all(|report| {
            report.level == DiagnosticLevel::Info
                && !report.installed
                && report.error_code.is_none()
        }));
    }
}
