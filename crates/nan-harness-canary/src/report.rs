use clap::ValueEnum;
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) const REPORT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryTrigger {
    Daily,
    Weekly,
    Release,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryTier {
    Installation,
    Deterministic,
    LiveCore,
    LiveExtended,
    ReleaseGate,
}

impl CanaryTier {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Installation => "installation",
            Self::Deterministic => "deterministic",
            Self::LiveCore => "live-core",
            Self::LiveExtended => "live-extended",
            Self::ReleaseGate => "release-gate",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CanaryOutcome {
    Passed,
    Failed,
    InfrastructureFailure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FailureClass {
    NanHarness,
    Harness,
    Installation,
    Provider,
    Infrastructure,
    TestContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NanHarnessEvidence {
    pub version: String,
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EnvironmentEvidence {
    pub operating_system: String,
    pub architecture: String,
    pub image: String,
    pub profile: String,
    #[serde(default)]
    pub runtimes: Vec<RuntimeEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RuntimeEvidence {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HarnessEvidence {
    pub id: HarnessKind,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckReport {
    pub name: String,
    pub status: CheckStatus,
    pub duration_milliseconds: u64,
    pub attempts: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FailureReport {
    pub class: FailureClass,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub summary: String,
    pub fingerprint: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanaryReport {
    pub schema_version: u8,
    pub run_id: String,
    pub cell_id: String,
    pub spec_sha256: String,
    pub trigger: CanaryTrigger,
    pub tier: CanaryTier,
    pub scenario: String,
    pub started_at: String,
    pub completed_at: String,
    pub duration_milliseconds: u64,
    pub nan_harness: NanHarnessEvidence,
    pub environment: EnvironmentEvidence,
    pub harness: HarnessEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub checks: Vec<CheckReport>,
    pub outcome: CanaryOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureReport>,
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
        if self.schema_version != REPORT_SCHEMA_VERSION {
            return Err(ReportError::UnsupportedSchema(self.schema_version));
        }
        for (field, value) in [
            ("runId", self.run_id.as_str()),
            ("cellId", self.cell_id.as_str()),
            ("specSha256", self.spec_sha256.as_str()),
            ("scenario", self.scenario.as_str()),
            ("startedAt", self.started_at.as_str()),
            ("completedAt", self.completed_at.as_str()),
            ("nanHarness.version", self.nan_harness.version.as_str()),
            ("nanHarness.source", self.nan_harness.source.as_str()),
            ("nanHarness.sha256", self.nan_harness.sha256.as_str()),
            (
                "environment.operatingSystem",
                self.environment.operating_system.as_str(),
            ),
            (
                "environment.architecture",
                self.environment.architecture.as_str(),
            ),
            ("environment.image", self.environment.image.as_str()),
            ("environment.profile", self.environment.profile.as_str()),
            ("harness.version", self.harness.version.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ReportError::EmptyField(field));
            }
        }
        if self.checks.is_empty() {
            return Err(ReportError::MissingChecks);
        }
        semver::Version::parse(&self.nan_harness.version)
            .map_err(|_| ReportError::InvalidSemanticVersion("nanHarness.version"))?;
        if self.outcome == CanaryOutcome::Passed {
            semver::Version::parse(&self.harness.version)
                .map_err(|_| ReportError::InvalidSemanticVersion("harness.version"))?;
        }
        for (field, value) in [
            ("specSha256", self.spec_sha256.as_str()),
            ("nanHarness.sha256", self.nan_harness.sha256.as_str()),
        ] {
            if !valid_sha256(value) {
                return Err(ReportError::InvalidSha256(field));
            }
        }
        let started_at = OffsetDateTime::parse(&self.started_at, &Rfc3339)
            .map_err(|_| ReportError::InvalidTimestamp("startedAt"))?;
        let completed_at = OffsetDateTime::parse(&self.completed_at, &Rfc3339)
            .map_err(|_| ReportError::InvalidTimestamp("completedAt"))?;
        if completed_at < started_at {
            return Err(ReportError::InvalidTimeOrder);
        }
        for check in &self.checks {
            if check.name.trim().is_empty() || check.attempts == 0 {
                return Err(ReportError::InvalidCheck);
            }
        }
        if self.outcome == CanaryOutcome::Passed && self.failure.is_some() {
            return Err(ReportError::UnexpectedFailure);
        }
        if self.outcome != CanaryOutcome::Passed && self.failure.is_none() {
            return Err(ReportError::MissingFailure);
        }
        let has_failed_check = self
            .checks
            .iter()
            .any(|check| check.status == CheckStatus::Failed);
        if (self.outcome == CanaryOutcome::Passed) == has_failed_check {
            return Err(ReportError::InconsistentChecks);
        }
        if self
            .failure
            .as_ref()
            .is_some_and(|failure| !valid_sha256(&failure.fingerprint))
        {
            return Err(ReportError::InvalidSha256("failure.fingerprint"));
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn failure_fingerprint(
    identity: &FailureIdentity<'_>,
    class: FailureClass,
    phase: &str,
    code: Option<&str>,
) -> String {
    let source = format!(
        "{}|{}|{}|{}|{}|{}|{:?}|{}|{}",
        identity.harness,
        identity.harness_version,
        identity.operating_system,
        identity.architecture,
        identity.tier.as_str(),
        identity.scenario,
        class,
        phase,
        code.unwrap_or_default()
    );
    sha256_hex(source.as_bytes())
}

pub(crate) fn sha256_hex(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[derive(Debug, Error)]
pub(crate) enum ReportError {
    #[error("could not read canary report '{}': {source}", path.display())]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse canary report '{}': {source}", path.display())]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not serialize canary report: {0}")]
    Serialize(serde_json::Error),
    #[error("could not create canary report directory '{}': {source}", path.display())]
    CreateDirectory {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write canary report '{}': {source}", path.display())]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("canary report path '{}' has no parent directory", .0.display())]
    InvalidPath(std::path::PathBuf),
    #[error("canary report schema {0} is unsupported")]
    UnsupportedSchema(u8),
    #[error("canary report field {0} must not be empty")]
    EmptyField(&'static str),
    #[error("canary report must contain at least one check")]
    MissingChecks,
    #[error("canary report field {0} must contain a semantic version")]
    InvalidSemanticVersion(&'static str),
    #[error("canary report field {0} must contain a lowercase or uppercase SHA-256 digest")]
    InvalidSha256(&'static str),
    #[error("canary report field {0} must contain an RFC 3339 timestamp")]
    InvalidTimestamp(&'static str),
    #[error("canary report completion time precedes its start time")]
    InvalidTimeOrder,
    #[error("canary report checks require a name and at least one attempt")]
    InvalidCheck,
    #[error("successful canary report must not contain a failure")]
    UnexpectedFailure,
    #[error("failed canary report must contain a failure")]
    MissingFailure,
    #[error("canary report outcome does not match its check statuses")]
    InconsistentChecks,
}

#[cfg(test)]
mod tests {
    use super::{
        CanaryOutcome, CanaryReport, CanaryTier, CanaryTrigger, CheckReport, CheckStatus,
        EnvironmentEvidence, FailureClass, FailureIdentity, FailureReport, HarnessEvidence,
        NanHarnessEvidence, REPORT_SCHEMA_VERSION,
    };
    use nan_harness_core::HarnessKind;

    fn report() -> CanaryReport {
        CanaryReport {
            schema_version: REPORT_SCHEMA_VERSION,
            run_id: "run-2026-08-22".to_owned(),
            cell_id: "linux-claude-live-read".to_owned(),
            spec_sha256: "b".repeat(64),
            trigger: CanaryTrigger::Daily,
            tier: CanaryTier::LiveCore,
            scenario: "read".to_owned(),
            started_at: "2026-08-22T08:00:00Z".to_owned(),
            completed_at: "2026-08-22T08:00:03Z".to_owned(),
            duration_milliseconds: 3_000,
            nan_harness: NanHarnessEvidence {
                version: "0.0.6".to_owned(),
                source: "release".to_owned(),
                sha256: "a".repeat(64),
            },
            environment: EnvironmentEvidence {
                operating_system: "linux".to_owned(),
                architecture: "aarch64".to_owned(),
                image: "ubuntu".to_owned(),
                profile: "node-24".to_owned(),
                runtimes: Vec::new(),
            },
            harness: HarnessEvidence {
                id: HarnessKind::ClaudeCode,
                version: "2.1.233".to_owned(),
            },
            model: Some("qwen3.6".to_owned()),
            checks: vec![CheckReport {
                name: "tool-read".to_owned(),
                status: CheckStatus::Passed,
                duration_milliseconds: 1_000,
                attempts: 1,
                detail: None,
            }],
            outcome: CanaryOutcome::Passed,
            failure: None,
        }
    }

    #[test]
    fn report_round_trips_atomically() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("report.json");
        let report = report();
        report.write(&path).expect("report should be written");
        assert_eq!(
            CanaryReport::read(&path).expect("report should load"),
            report
        );
    }

    #[test]
    fn passed_reports_require_semantic_versions() {
        let mut report = report();
        report.harness.version = "unknown".to_owned();

        assert!(matches!(
            report.validate(),
            Err(super::ReportError::InvalidSemanticVersion(
                "harness.version"
            ))
        ));
    }

    #[test]
    fn failure_fingerprint_is_stable_for_the_same_cell() {
        let identity = FailureIdentity {
            harness: HarnessKind::ClaudeCode,
            harness_version: "2.1.233",
            operating_system: "linux",
            architecture: "aarch64",
            tier: CanaryTier::LiveCore,
            scenario: "read",
        };
        let first = FailureReport::new(
            FailureClass::Harness,
            "tool",
            None,
            "first wording",
            &identity,
        );
        let second = FailureReport::new(
            FailureClass::Harness,
            "tool",
            None,
            "different wording",
            &identity,
        );
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn serialized_report_matches_the_documented_json_schema() {
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/canary-report.schema.json");
        let schema: serde_json::Value = serde_json::from_slice(
            &std::fs::read(schema_path).expect("canary report schema should be readable"),
        )
        .expect("canary report schema should be JSON");
        let validator = jsonschema::validator_for(&schema).expect("schema should compile");
        let value = serde_json::to_value(report()).expect("report should serialize");

        if let Err(error) = validator.validate(&value) {
            panic!("canary report should match its schema: {error}");
        }
    }
}
