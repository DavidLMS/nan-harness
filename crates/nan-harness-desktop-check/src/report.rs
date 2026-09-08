//! Allowlisted public evidence. Never serialize recovery state or diagnostic text here.

use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Read as _};
use std::path::Path;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const MAX_REPORT_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Platform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl Architecture {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else {
            Self::X86_64
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Passed,
    Failed,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    MissingKey,
    InvalidKey,
    MissingModel,
    InstallationUnavailable,
    InstallationFailed,
    InstallationAmbiguous,
    InstallationUnreadable,
    VersionUnknown,
    UnsupportedVersion,
    UnsupportedArchitecture,
    AlreadyRunning,
    PermissionRequired,
    LoginRequired,
    IsolationUnavailable,
    FocusChanged,
    WindowChanged,
    WindowOccluded,
    DesktopUnavailable,
    SelectorNotMatched,
    ActionUnsupported,
    InputMismatch,
    ResponseMismatch,
    ToolMismatch,
    ProviderFailed,
    BudgetExceeded,
    Cancelled,
    CleanupConflict,
    CleanupFailed,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckStep {
    Launched,
    InputSubmitted,
    ResponseVerified,
    ToolVerified,
    ErrorRecovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    Accessibility,
    AccessibilityAndKeyboard,
    VisualAndKeyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResponseVerification {
    Accessibility,
    LocalOcr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeResult {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub steps: Vec<CheckStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mode: Option<InputMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_verification: Option<ResponseVerification>,
    pub duration_milliseconds: u64,
}

impl ProbeResult {
    pub(crate) fn record_input(&mut self, mode: InputMode) {
        self.input_mode = Some(match (self.input_mode, mode) {
            (Some(InputMode::VisualAndKeyboard), _) | (_, InputMode::VisualAndKeyboard) => {
                InputMode::VisualAndKeyboard
            }
            (Some(InputMode::AccessibilityAndKeyboard), _)
            | (_, InputMode::AccessibilityAndKeyboard) => InputMode::AccessibilityAndKeyboard,
            _ => InputMode::Accessibility,
        });
    }

    pub(crate) fn record_response(&mut self, method: ResponseVerification) {
        if self.response_verification != Some(ResponseVerification::LocalOcr) {
            self.response_verification = Some(method);
        }
    }

    #[must_use]
    pub const fn not_run(reason: Reason) -> Self {
        Self {
            status: Status::Skipped,
            reason: Some(reason),
            steps: Vec::new(),
            input_mode: None,
            response_verification: None,
            duration_milliseconds: 0,
        }
    }

    #[must_use]
    pub fn blocked(reason: Reason) -> Self {
        Self {
            status: Status::Blocked,
            ..Self::not_run(reason)
        }
    }

    fn validate(&self, live: bool, schema_version: u8) -> Result<(), ReportError> {
        if schema_version == 1
            && (self.input_mode == Some(InputMode::VisualAndKeyboard)
                || self.response_verification.is_some())
        {
            return Err(ReportError::InvalidProbe);
        }
        let steps = self.steps.iter().copied().collect::<BTreeSet<_>>();
        if steps.len() != self.steps.len() || self.duration_milliseconds > 3_600_000 {
            return Err(ReportError::InvalidProbe);
        }
        if self.status != Status::Passed {
            return self
                .reason
                .map_or(Err(ReportError::InvalidProbe), |_| Ok(()));
        }
        let required = [
            CheckStep::Launched,
            CheckStep::InputSubmitted,
            CheckStep::ResponseVerified,
            CheckStep::ToolVerified,
        ];
        if self.reason.is_some()
            || self.input_mode.is_none()
            || (schema_version == 2 && self.response_verification.is_none())
            || required.iter().any(|step| !steps.contains(step))
            || (!live && !steps.contains(&CheckStep::ErrorRecovered))
        {
            return Err(ReportError::InvalidProbe);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BinaryIdentity {
    pub version: Version,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppResult {
    pub app: DesktopHarnessKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<Version>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<Version>,
    pub deterministic: [ProbeResult; 3],
    pub live: ProbeResult,
    pub cleanup: Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub schema_version: u8,
    pub checker_version: Version,
    pub run_id: String,
    pub started_at: String,
    pub platform: Platform,
    pub architecture: Architecture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nan_harness: Option<BinaryIdentity>,
    pub results: Vec<AppResult>,
    pub cleanup: Status,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("report cannot be read")]
    Io(#[from] io::Error),
    #[error("report is too large")]
    TooLarge,
    #[error("report does not match the public schema")]
    Schema,
    #[error("report identity is invalid")]
    Identity,
    #[error("report contains an invalid or duplicate application")]
    Applications,
    #[error("probe evidence is incomplete or inconsistent")]
    InvalidProbe,
}

impl Report {
    /// Read a bounded, allowlisted public report without exposing parser payloads.
    ///
    /// # Errors
    /// Fails on invalid evidence, unknown fields or an oversized document.
    pub fn read(path: &Path) -> Result<(Self, String), ReportError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_REPORT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }

    /// Parse evidence and return the digest of exactly the reviewed bytes.
    ///
    /// # Errors
    /// Rejects oversized, malformed or inconsistent evidence.
    pub fn parse(bytes: &[u8]) -> Result<(Self, String), ReportError> {
        if bytes.len() > MAX_REPORT_BYTES {
            return Err(ReportError::TooLarge);
        }
        let report: Self = serde_json::from_slice(bytes).map_err(|_| ReportError::Schema)?;
        report.validate()?;
        Ok((report, digest(bytes)))
    }

    /// Validate the evidence contract; version provenance is checked by the publisher.
    ///
    /// # Errors
    /// Rejects unsupported schema, invalid identity and incomplete passing probes.
    pub fn validate(&self) -> Result<(), ReportError> {
        if !matches!(self.schema_version, 1 | 2)
            || !hex_identifier(&self.run_id, 32)
            || OffsetDateTime::parse(&self.started_at, &Rfc3339).is_err()
            || self
                .nan_harness
                .as_ref()
                .is_some_and(|binary| !hex_identifier(&binary.sha256, 64))
        {
            return Err(ReportError::Identity);
        }
        let mut seen = BTreeSet::new();
        if self.results.is_empty() || self.results.len() > DesktopHarnessKind::ALL.len() {
            return Err(ReportError::Applications);
        }
        for app in &self.results {
            if !seen.insert(app.app) {
                return Err(ReportError::Applications);
            }
            for probe in &app.deterministic {
                probe.validate(false, self.schema_version)?;
            }
            app.live.validate(true, self.schema_version)?;
            if self.nan_harness.is_none()
                && (app.live.status == Status::Passed
                    || app
                        .deterministic
                        .iter()
                        .any(|probe| probe.status == Status::Passed))
            {
                return Err(ReportError::Identity);
            }
        }
        Ok(())
    }
}

fn hex_identifier(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            schema_version: 1,
            checker_version: Version::new(0, 1, 0),
            run_id: "a".repeat(32),
            started_at: "2026-09-08T00:00:00Z".into(),
            platform: Platform::Linux,
            architecture: Architecture::X86_64,
            nan_harness: None,
            results: vec![AppResult {
                app: DesktopHarnessKind::Zed,
                app_version: None,
                runtime_version: None,
                deterministic: std::array::from_fn(|_| {
                    ProbeResult::blocked(Reason::DesktopUnavailable)
                }),
                live: ProbeResult::not_run(Reason::MissingKey),
                cleanup: Status::Passed,
            }],
            cleanup: Status::Passed,
        }
    }

    #[test]
    fn visual_evidence_requires_v2_and_an_explicit_response_method() {
        let mut value = report();
        let probe = &mut value.results[0].live;
        probe.status = Status::Passed;
        probe.reason = None;
        probe.steps = vec![
            CheckStep::Launched,
            CheckStep::InputSubmitted,
            CheckStep::ResponseVerified,
            CheckStep::ToolVerified,
        ];
        probe.input_mode = Some(InputMode::VisualAndKeyboard);
        probe.response_verification = Some(ResponseVerification::LocalOcr);
        value.nan_harness = Some(BinaryIdentity {
            version: Version::new(0, 1, 2),
            sha256: "b".repeat(64),
        });
        assert!(matches!(value.validate(), Err(ReportError::InvalidProbe)));
        value.schema_version = 2;
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(Report::parse(&bytes).unwrap().0, value);
        value.results[0].live.response_verification = None;
        assert!(matches!(value.validate(), Err(ReportError::InvalidProbe)));
    }

    #[test]
    fn legacy_reports_remain_readable_without_inventing_verification() {
        let original = report();
        let bytes = serde_json::to_vec(&original).unwrap();
        let (decoded, hash) = Report::parse(&bytes).unwrap();
        assert_eq!(hash, digest(&bytes));
        assert_eq!(decoded, original);
        assert!(decoded.results[0].live.response_verification.is_none());
    }

    #[test]
    fn bounded_round_trip_binds_exact_bytes() {
        let value = report();
        let compact = serde_json::to_vec(&value).unwrap();
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        assert_eq!(Report::parse(&compact).unwrap().0, value);
        assert_ne!(
            Report::parse(&compact).unwrap().1,
            Report::parse(&pretty).unwrap().1
        );
        assert!(matches!(
            Report::parse(&vec![b' '; MAX_REPORT_BYTES + 1]),
            Err(ReportError::TooLarge)
        ));
    }

    #[test]
    fn rejects_private_fields_and_duplicate_apps() {
        let mut json = serde_json::to_value(report()).unwrap();
        json["apiKey"] = "must-not-be-accepted".into();
        assert!(matches!(
            Report::parse(&serde_json::to_vec(&json).unwrap()),
            Err(ReportError::Schema)
        ));
        let mut value = report();
        value.results.push(value.results[0].clone());
        assert!(matches!(value.validate(), Err(ReportError::Applications)));
    }

    #[test]
    fn passing_requires_behavior_not_a_success_label() {
        let mut value = report();
        let probe = &mut value.results[0].deterministic[0];
        probe.status = Status::Passed;
        probe.reason = None;
        assert!(matches!(value.validate(), Err(ReportError::InvalidProbe)));
        let probe = &mut value.results[0].deterministic[0];
        probe.input_mode = Some(InputMode::Accessibility);
        probe.steps = vec![
            CheckStep::Launched,
            CheckStep::InputSubmitted,
            CheckStep::ResponseVerified,
            CheckStep::ToolVerified,
            CheckStep::ErrorRecovered,
        ];
        assert!(matches!(value.validate(), Err(ReportError::Identity)));
        value.nan_harness = Some(BinaryIdentity {
            version: Version::new(0, 1, 2),
            sha256: "a".repeat(64),
        });
        assert!(value.validate().is_ok());
    }
}
