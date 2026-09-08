use super::super::discovery::DesktopDiscovery;
use super::super::models::{
    DOCTOR_SCHEMA_VERSION, DiagnosticLevel, ExperimentalHarnessDoctorReport,
    ExperimentalHarnessReport, ExperimentalTextReport,
};
use nan_harness_runtime::{DesktopCompatibilityEntry, DesktopCompatibilityEvidence};

pub(crate) fn experimental_json_report(
    entry: DesktopCompatibilityEntry,
) -> ExperimentalHarnessDoctorReport {
    let report = experimental_report(entry);
    ExperimentalHarnessDoctorReport {
        checks: report.checks,
        schema_version: DOCTOR_SCHEMA_VERSION,
        offline: false,
        harness: report.id,
        experimental: true,
        level: report.level,
        platform: report.platform,
        available: report.available,
        evidence: report.evidence,
        transport: report.transport,
        minimum_supported_version: report.minimum_supported_version,
        last_compatible_version: report.last_compatible_version,
        minimum_runtime_version: report.minimum_runtime_version,
        last_compatible_runtime_version: report.last_compatible_runtime_version,
        compatible_at: report.compatible_at,
        evidence_source: report.evidence_source,
        safe_to_share: report.safe_to_share,
    }
}

pub(crate) fn experimental_report(entry: DesktopCompatibilityEntry) -> ExperimentalHarnessReport {
    let available = entry.evidence != DesktopCompatibilityEvidence::Unavailable;
    ExperimentalHarnessReport {
        checks: entry.checks,
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
        minimum_runtime_version: entry
            .minimum_runtime_version
            .map(|version| version.to_string()),
        last_compatible_runtime_version: entry
            .last_compatible_runtime_version
            .map(|version| version.to_string()),
        compatible_at: entry.compatible_at,
        evidence_source: entry.source,
        safe_to_share: true,
    }
}

pub(super) fn experimental_json_reports(
    discoveries: Vec<DesktopDiscovery>,
) -> Vec<ExperimentalHarnessReport> {
    discoveries
        .into_iter()
        .filter_map(|(_, discovery)| discovery.ok().map(experimental_report))
        .collect()
}

pub(super) fn experimental_text_reports(
    discoveries: Vec<DesktopDiscovery>,
) -> Vec<ExperimentalTextReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| match discovery {
            Ok(entry) => ExperimentalTextReport::Available {
                checks: entry.checks,
                harness,
                platform: entry.platform,
                evidence: entry.evidence,
                transport: entry.transport,
                compatible_at: entry.compatible_at,
                evidence_source: entry.source,
            },
            Err(error) => ExperimentalTextReport::Failed {
                harness,
                error: error.to_string(),
            },
        })
        .collect()
}
