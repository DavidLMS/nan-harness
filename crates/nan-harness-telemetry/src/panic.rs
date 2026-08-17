use crate::consent::ReportConsent;
use crate::event::{ErrorReport, ErrorReportContext, Failure};
use crate::redaction::{SanitizedErrorReport, sanitize};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const MAX_PENDING_REPORT_BYTES: u64 = 64 * 1024;
const PENDING_REPORT_RETENTION: Duration = Duration::from_hours(168);

#[derive(Debug, Clone)]
pub struct PendingReportStore {
    path: PathBuf,
}

impl PendingReportStore {
    #[must_use]
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("pending-error-report.json"),
        }
    }

    /// Writes one bounded, already-sanitized report without authorizing transmission.
    ///
    /// # Errors
    ///
    /// Returns [`PendingReportError`] when the report cannot be serialized or persisted.
    pub fn save(&self, report: &SanitizedErrorReport) -> Result<(), PendingReportError> {
        let payload = serde_json::to_vec(report).map_err(PendingReportError::Serialize)?;
        if payload.len() as u64 > MAX_PENDING_REPORT_BYTES {
            return Err(PendingReportError::TooLarge(payload.len() as u64));
        }
        let directory = self
            .path
            .parent()
            .ok_or(PendingReportError::MissingParent)?;
        fs::create_dir_all(directory).map_err(PendingReportError::CreateDirectory)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(&self.path)
            .map_err(PendingReportError::Write)?;
        file.write_all(&payload)
            .map_err(PendingReportError::Write)?;
        file.sync_all().map_err(PendingReportError::Write)
    }

    /// Loads a pending report when it is valid, bounded, and within retention.
    ///
    /// # Errors
    ///
    /// Returns [`PendingReportError`] when the file cannot be inspected, read, or parsed.
    pub fn load(&self) -> Result<Option<ErrorReport>, PendingReportError> {
        self.load_at(OffsetDateTime::now_utc())
    }

    fn load_at(&self, now: OffsetDateTime) -> Result<Option<ErrorReport>, PendingReportError> {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PendingReportError::Read(error)),
        };
        if metadata.len() > MAX_PENDING_REPORT_BYTES {
            let _ = self.delete();
            return Ok(None);
        }
        let payload = fs::read(&self.path).map_err(PendingReportError::Read)?;
        let report: ErrorReport = serde_json::from_slice(&payload).map_err(|error| {
            let _ = self.delete();
            PendingReportError::Parse(error)
        })?;
        let created = OffsetDateTime::parse(report.timestamp(), &Rfc3339).map_err(|error| {
            let _ = self.delete();
            PendingReportError::Timestamp(error)
        })?;
        let retention = time::Duration::try_from(PENDING_REPORT_RETENTION)
            .expect("the pending report retention fits time::Duration");
        if now - created > retention {
            let _ = self.delete();
            return Ok(None);
        }
        if sanitize(report.clone()).is_err() {
            let _ = self.delete();
            return Ok(None);
        }
        Ok(Some(report))
    }

    /// Deletes the pending local report.
    ///
    /// # Errors
    ///
    /// Returns [`PendingReportError`] when an existing file cannot be removed.
    pub fn delete(&self) -> Result<(), PendingReportError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(PendingReportError::Delete(error)),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn install_panic_hook(store: PendingReportStore, telemetry_enabled: bool, interactive: bool) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |information| {
        let consent = if telemetry_enabled {
            ReportConsent::automatic()
        } else {
            ReportConsent::one_time()
        };
        let context = ErrorReportContext::new(Failure::panic(), interactive);
        if let Ok(report) = ErrorReport::new(context, consent)
            && let Ok(report) = sanitize(report)
        {
            let _ = store.save(&report);
        }
        previous(information);
    }));
}

#[derive(Debug, Error)]
pub enum PendingReportError {
    #[error("pending report path has no parent directory")]
    MissingParent,
    #[error("could not create the pending report directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("could not serialize the pending report: {0}")]
    Serialize(serde_json::Error),
    #[error("pending report is too large: {0} bytes")]
    TooLarge(u64),
    #[error("could not write the pending report: {0}")]
    Write(std::io::Error),
    #[error("could not read the pending report: {0}")]
    Read(std::io::Error),
    #[error("pending report is not valid JSON: {0}")]
    Parse(serde_json::Error),
    #[error("pending report timestamp is invalid: {0}")]
    Timestamp(time::error::Parse),
    #[error("could not delete the pending report: {0}")]
    Delete(std::io::Error),
}
