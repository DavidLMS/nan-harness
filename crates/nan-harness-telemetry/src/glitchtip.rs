use crate::redaction::SanitizedErrorReport;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_EXPORT_TIMEOUT: Duration = Duration::from_secs(2);

pub type ExportFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ExportError>> + Send + 'a>>;

pub trait ErrorReportExporter: Send + Sync {
    fn export<'a>(&'a self, report: &'a SanitizedErrorReport) -> ExportFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct GlitchTipExporter {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    public_dsn: String,
    authorization: String,
}

impl GlitchTipExporter {
    /// Builds a bounded Sentry-compatible exporter from a `GlitchTip` project DSN.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError`] when the DSN is invalid or the HTTP client cannot be built.
    pub fn new(dsn: &str, timeout: Duration) -> Result<Self, ExportError> {
        let parsed = ParsedDsn::parse(dsn)?;
        let client = reqwest::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .user_agent(concat!("nan-harness/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ExportError::BuildClient)?;
        Ok(Self {
            client,
            endpoint: parsed.endpoint,
            public_dsn: parsed.public_dsn,
            authorization: format!(
                "Sentry sentry_version=7,sentry_client=nan-harness/{},sentry_key={}",
                env!("CARGO_PKG_VERSION"),
                parsed.public_key
            ),
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &reqwest::Url {
        &self.endpoint
    }

    async fn send(&self, report: &SanitizedErrorReport) -> Result<(), ExportError> {
        let envelope = envelope(report, &self.public_dsn)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "application/x-sentry-envelope")
            .header("X-Sentry-Auth", &self.authorization)
            .body(envelope)
            .send()
            .await
            .map_err(ExportError::Request)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ExportError::Status(response.status()))
        }
    }
}

impl ErrorReportExporter for GlitchTipExporter {
    fn export<'a>(&'a self, report: &'a SanitizedErrorReport) -> ExportFuture<'a> {
        Box::pin(self.send(report))
    }
}

#[derive(Debug)]
struct ParsedDsn {
    endpoint: reqwest::Url,
    public_dsn: String,
    public_key: String,
}

impl ParsedDsn {
    fn parse(value: &str) -> Result<Self, ExportError> {
        let mut dsn = reqwest::Url::parse(value).map_err(ExportError::InvalidDsn)?;
        if !matches!(dsn.scheme(), "http" | "https")
            || dsn.username().is_empty()
            || dsn.password().is_some()
            || dsn.query().is_some()
            || dsn.fragment().is_some()
        {
            return Err(ExportError::UnsupportedDsn);
        }
        let public_key = dsn.username().to_owned();
        let mut segments = dsn
            .path_segments()
            .ok_or(ExportError::UnsupportedDsn)?
            .filter(|segment| !segment.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let project_id = segments.pop().ok_or(ExportError::UnsupportedDsn)?;
        if !project_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ExportError::UnsupportedDsn);
        }
        let public_dsn = dsn.to_string();
        dsn.set_username("")
            .map_err(|()| ExportError::UnsupportedDsn)?;
        dsn.set_password(None)
            .map_err(|()| ExportError::UnsupportedDsn)?;
        let prefix = if segments.is_empty() {
            String::new()
        } else {
            format!("/{}/", segments.join("/"))
        };
        dsn.set_path(&format!("{prefix}api/{project_id}/envelope/"));
        Ok(Self {
            endpoint: dsn,
            public_dsn,
            public_key,
        })
    }
}

fn envelope(report: &SanitizedErrorReport, dsn: &str) -> Result<Vec<u8>, ExportError> {
    let report = report.as_report();
    let event_id = report
        .report_id()
        .strip_prefix("report_")
        .ok_or(ExportError::InvalidReportId)?;
    let event = sentry_event(report, event_id)?;
    let event_payload = serde_json::to_vec(&event).map_err(ExportError::Serialize)?;
    let envelope_header = serde_json::to_vec(&json!({
        "event_id": event_id,
        "dsn": dsn,
        "sent_at": report.timestamp()
    }))
    .map_err(ExportError::Serialize)?;
    let item_header = serde_json::to_vec(&json!({
        "type": "event",
        "content_type": "application/json",
        "length": event_payload.len()
    }))
    .map_err(ExportError::Serialize)?;
    let mut payload =
        Vec::with_capacity(envelope_header.len() + item_header.len() + event_payload.len() + 3);
    payload.extend(envelope_header);
    payload.push(b'\n');
    payload.extend(item_header);
    payload.push(b'\n');
    payload.extend(event_payload);
    payload.push(b'\n');
    Ok(payload)
}

fn sentry_event(report: &crate::event::ErrorReport, event_id: &str) -> Result<Value, ExportError> {
    let mut tags = BTreeMap::from([
        ("error.code", report.failure().code().to_owned()),
        (
            "error.category",
            report.failure().category().as_str().to_owned(),
        ),
        ("error.stage", report.failure().stage().as_str().to_owned()),
        ("error.panic", report.failure().is_panic().to_string()),
        ("error.retryable", report.failure().retryable().to_string()),
        (
            "runtime.os_family",
            report.runtime().os_family().as_str().to_owned(),
        ),
        (
            "runtime.architecture",
            report.runtime().architecture().as_str().to_owned(),
        ),
        (
            "runtime.target_environment",
            report.runtime().target_environment().as_str().to_owned(),
        ),
        (
            "runtime.interactive",
            report.runtime().interactive().to_string(),
        ),
        ("consent.mode", report.consent().mode().as_str().to_owned()),
        (
            "consent.telemetry_enabled",
            report.consent().telemetry_enabled().to_string(),
        ),
    ]);
    if let Some(commit) = report.application().build_commit() {
        tags.insert("application.build_commit", commit.to_owned());
    }
    if let Some(cause) = report.failure().cause() {
        tags.insert("error.cause", cause.as_str().to_owned());
    }
    if let Some(status) = report.failure().http_status() {
        tags.insert("error.http_status", status.to_string());
    }
    if let Some(harness) = report.harness() {
        tags.insert("harness.kind", harness.kind().as_str().to_owned());
        if let Some(version) = harness.version() {
            tags.insert("harness.version", version.to_owned());
        }
        if let Some(compatibility) = harness.compatibility() {
            tags.insert("harness.compatibility", compatibility.as_str().to_owned());
        }
    }
    if let Some(transport) = report.transport() {
        tags.insert("transport", transport.as_str().to_owned());
    }
    if let Some(operation) = report.operation() {
        tags.insert("operation.kind", operation.kind().as_str().to_owned());
    }
    let safe_context = serde_json::to_value(report).map_err(ExportError::Serialize)?;
    Ok(json!({
        "event_id": event_id,
        "timestamp": report.timestamp(),
        "platform": "native",
        "level": "error",
        "logger": "nan-harness",
        "release": report.application().version(),
        "message": format!("NaN Harness error {}", report.failure().code()),
        "fingerprint": [report.failure().code()],
        "tags": tags,
        "contexts": {
            "nan_harness": safe_context
        }
    }))
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("GlitchTip DSN is not a valid URL: {0}")]
    InvalidDsn(url::ParseError),
    #[error("GlitchTip DSN must use HTTP(S), a public key, and a numeric project ID")]
    UnsupportedDsn,
    #[error("could not build the bounded GlitchTip client: {0}")]
    BuildClient(reqwest::Error),
    #[error("could not serialize the GlitchTip envelope: {0}")]
    Serialize(serde_json::Error),
    #[error("error report identifier cannot be converted into a Sentry event ID")]
    InvalidReportId,
    #[error("GlitchTip request failed: {0}")]
    Request(reqwest::Error),
    #[error("GlitchTip rejected the error report with HTTP {0}")]
    Status(StatusCode),
}
