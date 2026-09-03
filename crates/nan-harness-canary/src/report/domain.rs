use super::fingerprint::failure_fingerprint;
use super::{CanaryReport, CanaryTier, FailureClass, FailureReport, ReportError, validation};
use nan_harness_core::HarnessKind;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;

impl FailureReport {
    pub(crate) fn new(
        class: FailureClass,
        phase: impl Into<String>,
        code: Option<String>,
        summary: impl Into<String>,
        identity: &FailureIdentity<'_>,
    ) -> Self {
        let phase = phase.into();
        let summary = summary.into();
        let fingerprint = failure_fingerprint(identity, class, &phase, code.as_deref());
        Self {
            class,
            phase,
            code,
            summary,
            fingerprint,
        }
    }
}

pub(crate) struct FailureIdentity<'a> {
    pub harness: HarnessKind,
    pub harness_version: &'a str,
    pub operating_system: &'a str,
    pub architecture: &'a str,
    pub tier: CanaryTier,
    pub scenario: &'a str,
}

impl CanaryReport {
    pub(crate) fn read(path: &Path) -> Result<Self, ReportError> {
        let contents = fs::read(path).map_err(|source| ReportError::Read {
            path: path.to_owned(),
            source,
        })?;
        let report = serde_json::from_slice(&contents).map_err(|source| ReportError::Parse {
            path: path.to_owned(),
            source,
        })?;
        Ok(report)
    }

    pub(crate) fn write(&self, path: &Path) -> Result<(), ReportError> {
        self.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| ReportError::InvalidPath(path.to_owned()))?;
        fs::create_dir_all(parent).map_err(|source| ReportError::CreateDirectory {
            path: parent.to_owned(),
            source,
        })?;
        let payload = serde_json::to_vec_pretty(self).map_err(ReportError::Serialize)?;
        let mut temporary = TempFileBuilder::new()
            .prefix(".nan-harness-canary-")
            .tempfile_in(parent)
            .map_err(|source| ReportError::Write {
                path: path.to_owned(),
                source,
            })?;
        temporary
            .write_all(&payload)
            .and_then(|()| temporary.write_all(b"\n"))
            .and_then(|()| temporary.flush())
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| ReportError::Write {
                path: path.to_owned(),
                source,
            })?;
        temporary
            .persist(path)
            .map_err(|error| ReportError::Write {
                path: path.to_owned(),
                source: error.error,
            })?;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), ReportError> {
        validation::validate(self)
    }
}
