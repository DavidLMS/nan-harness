use super::{CanaryOutcome, CanaryReport, CheckStatus, REPORT_SCHEMA_VERSION, ReportError};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const LEGACY_REPORT_SCHEMA_VERSION: u8 = 1;
const MAX_OBSERVATIONS: usize = 1;

pub(super) fn validate(report: &CanaryReport) -> Result<(), ReportError> {
    if !matches!(
        report.schema_version,
        LEGACY_REPORT_SCHEMA_VERSION | REPORT_SCHEMA_VERSION
    ) {
        return Err(ReportError::UnsupportedSchema(report.schema_version));
    }
    if report.schema_version == LEGACY_REPORT_SCHEMA_VERSION && !report.observations.is_empty() {
        return Err(ReportError::LegacyObservations);
    }
    if report.observations.len() > MAX_OBSERVATIONS {
        return Err(ReportError::TooManyObservations(report.observations.len()));
    }
    if report
        .observations
        .iter()
        .any(|observation| !valid_sha256(&observation.fingerprint))
    {
        return Err(ReportError::InvalidSha256("observations.fingerprint"));
    }
    for (field, value) in [
        ("runId", report.run_id.as_str()),
        ("cellId", report.cell_id.as_str()),
        ("specSha256", report.spec_sha256.as_str()),
        ("scenario", report.scenario.as_str()),
        ("startedAt", report.started_at.as_str()),
        ("completedAt", report.completed_at.as_str()),
        ("nanHarness.version", report.nan_harness.version.as_str()),
        ("nanHarness.source", report.nan_harness.source.as_str()),
        ("nanHarness.sha256", report.nan_harness.sha256.as_str()),
        (
            "environment.operatingSystem",
            report.environment.operating_system.as_str(),
        ),
        (
            "environment.architecture",
            report.environment.architecture.as_str(),
        ),
        ("environment.image", report.environment.image.as_str()),
        ("environment.profile", report.environment.profile.as_str()),
        ("harness.version", report.harness.version.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ReportError::EmptyField(field));
        }
    }
    if report.checks.is_empty() {
        return Err(ReportError::MissingChecks);
    }
    semver::Version::parse(&report.nan_harness.version)
        .map_err(|_| ReportError::InvalidSemanticVersion("nanHarness.version"))?;
    if report.outcome == CanaryOutcome::Passed || report.harness.version != "unknown" {
        semver::Version::parse(&report.harness.version)
            .map_err(|_| ReportError::InvalidSemanticVersion("harness.version"))?;
    }
    for (field, value) in [
        ("specSha256", report.spec_sha256.as_str()),
        ("nanHarness.sha256", report.nan_harness.sha256.as_str()),
    ] {
        if !valid_sha256(value) {
            return Err(ReportError::InvalidSha256(field));
        }
    }
    let started_at = OffsetDateTime::parse(&report.started_at, &Rfc3339)
        .map_err(|_| ReportError::InvalidTimestamp("startedAt"))?;
    let completed_at = OffsetDateTime::parse(&report.completed_at, &Rfc3339)
        .map_err(|_| ReportError::InvalidTimestamp("completedAt"))?;
    if completed_at < started_at {
        return Err(ReportError::InvalidTimeOrder);
    }
    for check in &report.checks {
        if check.name.trim().is_empty() || check.attempts == 0 {
            return Err(ReportError::InvalidCheck);
        }
    }
    if report.outcome == CanaryOutcome::Passed && report.failure.is_some() {
        return Err(ReportError::UnexpectedFailure);
    }
    if report.outcome != CanaryOutcome::Passed && report.failure.is_none() {
        return Err(ReportError::MissingFailure);
    }
    let has_failed_check = report
        .checks
        .iter()
        .any(|check| check.status == CheckStatus::Failed);
    if (report.outcome == CanaryOutcome::Passed) == has_failed_check {
        return Err(ReportError::InconsistentChecks);
    }
    if report
        .failure
        .as_ref()
        .is_some_and(|failure| !valid_sha256(&failure.fingerprint))
    {
        return Err(ReportError::InvalidSha256("failure.fingerprint"));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
