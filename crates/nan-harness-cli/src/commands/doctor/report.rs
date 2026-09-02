use super::discovery::{
    DesktopDiscovery, IntegrationDiscovery, ProviderDiscovery, SystemDiscovery, TelemetryDiscovery,
};
use super::models::{
    ConfigurationTextReport, DiagnosticLevel, ExperimentalHarnessDoctorReport,
    ExperimentalHarnessReport, ExperimentalTextReport, HarnessDetails, HarnessDoctorReport,
    HarnessReport, HarnessTextReport, HarnessTextStatus, IntegrationReport, IntegrationSection,
    PlatformReport, ProviderReport, ProviderTextReport, SystemDoctorReport, TelemetryReport,
    TelemetryTextReport, TextSystemReport, coding_model_summaries,
};
use nan_harness_core::{HarnessKind, VersionStatus};
use nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence;
use nan_harness_runtime::{DiscoveryError, DiscoveryReport};

pub(crate) fn system_json_report(discovery: SystemDiscovery) -> SystemDoctorReport {
    SystemDoctorReport {
        schema_version: super::models::DOCTOR_SCHEMA_VERSION,
        nan_harness_version: env!("CARGO_PKG_VERSION"),
        platform: PlatformReport {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        provider: provider_json_report(discovery.provider),
        harnesses: harness_json_reports(discovery.harnesses),
        experimental_harnesses: experimental_json_reports(discovery.experimental_harnesses),
        managed_configurations: integration_json_report(discovery.managed_configurations),
        telemetry: telemetry_json_report(discovery.telemetry),
        safe_to_share: true,
    }
}

pub(crate) fn system_text_report(discovery: SystemDiscovery) -> TextSystemReport {
    TextSystemReport {
        provider: provider_text_report(discovery.provider),
        harnesses: harness_text_reports(discovery.harnesses),
        experimental_harnesses: experimental_text_reports(discovery.experimental_harnesses),
        managed_configurations: configuration_text_report(discovery.managed_configurations),
        telemetry: telemetry_text_report(discovery.telemetry),
    }
}

pub(crate) fn harness_json_report(
    harness: HarnessKind,
    discovery: Result<DiscoveryReport, DiscoveryError>,
) -> HarnessDoctorReport {
    match discovery {
        Ok(discovery) => HarnessDoctorReport {
            schema_version: super::models::DOCTOR_SCHEMA_VERSION,
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
            schema_version: super::models::DOCTOR_SCHEMA_VERSION,
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

pub(crate) fn experimental_json_report(
    entry: nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEntry,
) -> ExperimentalHarnessDoctorReport {
    let report = experimental_report(entry);
    ExperimentalHarnessDoctorReport {
        schema_version: super::models::DOCTOR_SCHEMA_VERSION,
        harness: report.id,
        experimental: true,
        level: report.level,
        platform: report.platform,
        available: report.available,
        evidence: report.evidence,
        transport: report.transport,
        minimum_supported_version: report.minimum_supported_version,
        last_compatible_version: report.last_compatible_version,
        compatible_at: report.compatible_at,
        safe_to_share: report.safe_to_share,
    }
}

pub(crate) fn experimental_report(
    entry: nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEntry,
) -> ExperimentalHarnessReport {
    let available = entry.evidence != DesktopCompatibilityEvidence::Unavailable;
    ExperimentalHarnessReport {
        id: entry.id,
        level: if available {
            DiagnosticLevel::Warning
        } else {
            DiagnosticLevel::Info
        },
        platform: entry.platform,
        available,
        evidence: entry.evidence,
        transport: entry.transport,
        minimum_supported_version: entry.minimum_app_version.map(|version| version.to_string()),
        last_compatible_version: entry
            .last_compatible_app_version
            .map(|version| version.to_string()),
        compatible_at: entry.compatible_at,
        safe_to_share: true,
    }
}

fn experimental_json_reports(discoveries: Vec<DesktopDiscovery>) -> Vec<ExperimentalHarnessReport> {
    discoveries
        .into_iter()
        .filter_map(|(_, discovery)| discovery.ok().map(experimental_report))
        .collect()
}

fn provider_json_report(discovery: ProviderDiscovery) -> ProviderReport {
    match discovery {
        ProviderDiscovery::NotConfigured => ProviderReport {
            level: DiagnosticLevel::Info,
            credential: "not-configured",
            api: "skipped",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::Invalid(code) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "invalid",
            api: "skipped",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some(code),
        },
        ProviderDiscovery::Models(models) => ProviderReport {
            level: DiagnosticLevel::Ok,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(models.len()),
            coding_models: coding_model_summaries(models),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::NoModels => ProviderReport {
            level: DiagnosticLevel::Warning,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(0),
            coding_models: Vec::new(),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::Status(status) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: if matches!(status, 401 | 403) {
                "authentication-rejected"
            } else {
                "request-rejected"
            },
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: Some(status),
            error_code: Some("NH-PERSISTENCE-003"),
        },
        ProviderDiscovery::InvalidResponse => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "invalid-response",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-004"),
        },
        ProviderDiscovery::Unavailable(code) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "unavailable",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some(code),
        },
        ProviderDiscovery::Timeout => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "timeout",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-002"),
        },
    }
}

fn provider_text_report(discovery: ProviderDiscovery) -> ProviderTextReport {
    match discovery {
        ProviderDiscovery::NotConfigured => ProviderTextReport::NotConfigured,
        ProviderDiscovery::Invalid(code) => ProviderTextReport::Invalid(code),
        ProviderDiscovery::Models(models) => ProviderTextReport::Models(models),
        ProviderDiscovery::NoModels => ProviderTextReport::NoModels,
        ProviderDiscovery::Status(status) => ProviderTextReport::Status(status),
        ProviderDiscovery::InvalidResponse => ProviderTextReport::InvalidResponse,
        ProviderDiscovery::Unavailable(code) => ProviderTextReport::Unavailable(code),
        ProviderDiscovery::Timeout => ProviderTextReport::Timeout,
    }
}

fn harness_json_reports(
    discoveries: Vec<(HarnessKind, Result<DiscoveryReport, DiscoveryError>)>,
) -> Vec<HarnessReport> {
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

fn harness_text_reports(
    discoveries: Vec<(HarnessKind, Result<DiscoveryReport, DiscoveryError>)>,
) -> Vec<HarnessTextReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| {
            let status = match discovery {
                Ok(discovery) => {
                    let version = normalized_version(&discovery.harness.detected_version)
                        .unwrap_or_else(|| "unparseable".to_owned());
                    let (level, label) = match discovery.harness.version_status {
                        VersionStatus::Tested => ("OK", "tested"),
                        VersionStatus::Supported => ("OK", "supported"),
                        VersionStatus::NewerUntested => ("WARN", "newer than compatible"),
                        VersionStatus::OlderUnsupported => ("ERROR", "unsupported"),
                        VersionStatus::Unparseable => ("WARN", "version unparseable"),
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

fn experimental_text_reports(discoveries: Vec<DesktopDiscovery>) -> Vec<ExperimentalTextReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| match discovery {
            Ok(entry) => ExperimentalTextReport::Available {
                harness,
                platform: entry.platform,
                evidence: entry.evidence,
                transport: entry.transport,
            },
            Err(error) => ExperimentalTextReport::Failed {
                harness,
                error: error.to_string(),
            },
        })
        .collect()
}

fn integration_json_report(discovery: IntegrationDiscovery) -> IntegrationSection {
    match discovery {
        IntegrationDiscovery::Failed { code, .. } => IntegrationSection {
            level: DiagnosticLevel::Error,
            integrations: Vec::new(),
            error_code: Some(code),
        },
        IntegrationDiscovery::Configured(integrations) => {
            let integrations = integrations
                .into_iter()
                .map(|integration| IntegrationReport {
                    id: integration.id,
                    active: integration.active,
                })
                .collect::<Vec<_>>();
            let level = if integrations.iter().all(|integration| integration.active) {
                DiagnosticLevel::Info
            } else {
                DiagnosticLevel::Warning
            };
            IntegrationSection {
                level,
                integrations,
                error_code: None,
            }
        }
    }
}

fn configuration_text_report(discovery: IntegrationDiscovery) -> ConfigurationTextReport {
    match discovery {
        IntegrationDiscovery::Failed {
            subject,
            status,
            code,
        } => ConfigurationTextReport::Failed {
            subject,
            status,
            code,
        },
        IntegrationDiscovery::Configured(integrations) if integrations.is_empty() => {
            ConfigurationTextReport::NoneConfigured
        }
        IntegrationDiscovery::Configured(integrations) => ConfigurationTextReport::Configured(
            integrations
                .into_iter()
                .map(|integration| IntegrationReport {
                    id: integration.id,
                    active: integration.active,
                })
                .collect(),
        ),
    }
}

fn telemetry_json_report(discovery: TelemetryDiscovery) -> TelemetryReport {
    match discovery {
        TelemetryDiscovery::State(enabled) => TelemetryReport {
            level: DiagnosticLevel::Info,
            enabled: Some(enabled),
            error_code: None,
        },
        TelemetryDiscovery::Failed => TelemetryReport {
            level: DiagnosticLevel::Error,
            enabled: None,
            error_code: Some("NH-TELEMETRY-001"),
        },
    }
}

fn telemetry_text_report(discovery: TelemetryDiscovery) -> TelemetryTextReport {
    match discovery {
        TelemetryDiscovery::State(enabled) => TelemetryTextReport::State(enabled),
        TelemetryDiscovery::Failed => TelemetryTextReport::Failed,
    }
}

fn diagnostic_level(status: VersionStatus) -> DiagnosticLevel {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => DiagnosticLevel::Ok,
        VersionStatus::NewerUntested | VersionStatus::Unparseable => DiagnosticLevel::Warning,
        VersionStatus::OlderUnsupported => DiagnosticLevel::Error,
    }
}

pub(crate) fn harness_details(discovery: DiscoveryReport) -> HarnessDetails {
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
        compatibility: compatibility_label(discovery.harness.version_status),
        warnings: discovery.warnings,
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

        assert_eq!(super::super::models::DOCTOR_SCHEMA_VERSION, 5);
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
