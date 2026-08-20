use crate::event::ErrorReport;
use serde::Serialize;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const MAX_STACK_FRAMES: usize = 32;

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SanitizedErrorReport(ErrorReport);

impl SanitizedErrorReport {
    #[must_use]
    pub fn as_report(&self) -> &ErrorReport {
        &self.0
    }
}

/// Enforces the final privacy allowlist before a report can reach an exporter.
///
/// # Errors
///
/// Returns [`RedactionError`] when any field falls outside the closed telemetry contract.
pub fn sanitize(report: ErrorReport) -> Result<SanitizedErrorReport, RedactionError> {
    if !matches!(report.schema_version(), 1 | 2) {
        return Err(RedactionError::SchemaVersion);
    }
    validate_report_id(report.report_id())?;
    validate_timestamp(report.timestamp())?;
    if report.application().name() != "nan-harness" {
        return Err(RedactionError::ApplicationName);
    }
    validate_metadata("application.version", report.application().version(), 64)?;
    if let Some(commit) = report.application().build_commit() {
        validate_commit(commit)?;
    }
    validate_error_code(report.failure().code())?;
    let cause = report.failure().cause();
    if let Some(status) = report.failure().http_status() {
        if cause != Some(crate::event::FailureCause::HttpStatus) || !(100..=599).contains(&status) {
            return Err(RedactionError::FailureDiagnostics);
        }
    } else if cause == Some(crate::event::FailureCause::HttpStatus) {
        return Err(RedactionError::FailureDiagnostics);
    }
    if !report.consent().is_valid() {
        return Err(RedactionError::Consent);
    }
    if let Some(version) = report
        .harness()
        .and_then(crate::event::HarnessIdentity::version)
    {
        validate_metadata("harness.version", version, 64)?;
    }
    if report.stack().len() > MAX_STACK_FRAMES {
        return Err(RedactionError::StackLength(report.stack().len()));
    }
    for frame in report.stack() {
        validate_symbol("stack.module", frame.module(), 160)?;
        validate_symbol("stack.function", frame.function(), 240)?;
    }
    Ok(SanitizedErrorReport(report))
}

fn validate_commit(value: &str) -> Result<(), RedactionError> {
    if (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(RedactionError::ForbiddenValue {
            field: "application.buildCommit",
        })
    }
}

fn validate_report_id(value: &str) -> Result<(), RedactionError> {
    let Some(identifier) = value.strip_prefix("report_") else {
        return Err(RedactionError::ReportId);
    };
    if (12..=64).contains(&identifier.len())
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(RedactionError::ReportId)
    }
}

fn validate_timestamp(value: &str) -> Result<(), RedactionError> {
    if value.ends_with('Z') && OffsetDateTime::parse(value, &Rfc3339).is_ok() {
        Ok(())
    } else {
        Err(RedactionError::Timestamp)
    }
}

fn validate_error_code(value: &str) -> Result<(), RedactionError> {
    if (6..=51).contains(&value.len())
        && value.starts_with("NH-")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(RedactionError::ErrorCode)
    }
}

fn validate_metadata(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), RedactionError> {
    if !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        Ok(())
    } else {
        Err(RedactionError::ForbiddenValue { field })
    }
}

fn validate_symbol(field: &'static str, value: &str, maximum: usize) -> Result<(), RedactionError> {
    if !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b':' | b'.' | b'-' | b'<' | b'>' | b'{' | b'}')
        })
    {
        Ok(())
    } else {
        Err(RedactionError::ForbiddenValue { field })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedactionError {
    #[error("unsupported error-report schema version")]
    SchemaVersion,
    #[error("invalid error-report identifier")]
    ReportId,
    #[error("invalid error-report timestamp")]
    Timestamp,
    #[error("invalid application name")]
    ApplicationName,
    #[error("invalid error code")]
    ErrorCode,
    #[error("invalid consent invariant")]
    Consent,
    #[error("invalid failure diagnostics invariant")]
    FailureDiagnostics,
    #[error("error report contains {0} stack frames; at most 32 are allowed")]
    StackLength(usize),
    #[error("field '{field}' contains a value outside the telemetry allowlist")]
    ForbiddenValue { field: &'static str },
}
