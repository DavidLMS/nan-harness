use crate::app::AggregateArgs;
use crate::report::{CanaryOutcome, CanaryReport, FailureClass, ReportError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const STATE_SCHEMA_VERSION: u8 = 1;
const SUMMARY_SCHEMA_VERSION: u8 = 1;

pub(crate) fn run(arguments: &AggregateArgs) -> Result<(), AggregateError> {
    let mut reports = read_reports(&arguments.reports)?;
    reports.sort_by(|left, right| {
        left.completed_at
            .cmp(&right.completed_at)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });

    let mut state = AggregateState::read_or_default(&arguments.state)?;
    let mut alerts = Vec::new();
    let mut processed = 0_usize;
    for report in reports {
        report.validate()?;
        if state.observe(&report, &mut alerts) {
            processed += 1;
        }
    }
    state.updated_at = timestamp()?;
    state.write(&arguments.state)?;

    let summary = AggregateSummary {
        schema_version: SUMMARY_SCHEMA_VERSION,
        generated_at: state.updated_at.clone(),
        processed_reports: processed,
        tracked_cells: state.cells.len(),
        suspected_failures: state
            .cells
            .values()
            .filter(|cell| cell.consecutive_failures == 1)
            .count(),
        confirmed_failures: state
            .cells
            .values()
            .filter(|cell| cell.consecutive_failures >= 2)
            .count(),
        alerts,
    };
    atomic_json_write(&arguments.summary, &summary)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(AggregateError::Serialize)?
    );
    Ok(())
}

fn read_reports(directory: &Path) -> Result<Vec<CanaryReport>, AggregateError> {
    let entries = fs::read_dir(directory).map_err(|source| AggregateError::ReadDirectory {
        path: directory.to_owned(),
        source,
    })?;
    let mut reports = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| AggregateError::ReadDirectory {
            path: directory.to_owned(),
            source,
        })?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            reports.push(CanaryReport::read(&path)?);
        }
    }
    if reports.is_empty() {
        return Err(AggregateError::NoReports(directory.to_owned()));
    }
    Ok(reports)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AggregateState {
    schema_version: u8,
    updated_at: String,
    #[serde(default)]
    cells: BTreeMap<String, CellState>,
}

impl Default for AggregateState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            updated_at: String::new(),
            cells: BTreeMap::new(),
        }
    }
}

impl AggregateState {
    fn read_or_default(path: &Path) -> Result<Self, AggregateError> {
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(AggregateError::ReadState {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let state: Self =
            serde_json::from_slice(&contents).map_err(|source| AggregateError::ParseState {
                path: path.to_owned(),
                source,
            })?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(AggregateError::UnsupportedStateSchema(state.schema_version));
        }
        Ok(state)
    }

    fn write(&self, path: &Path) -> Result<(), AggregateError> {
        atomic_json_write(path, self)
    }

    fn observe(&mut self, report: &CanaryReport, alerts: &mut Vec<AggregateAlert>) -> bool {
        let key = cell_key(report);
        let cell = self.cells.entry(key).or_default();
        if !cell.last_completed_at.is_empty() && report.completed_at <= cell.last_completed_at {
            return false;
        }

        match report.outcome {
            CanaryOutcome::Passed => {
                if cell.consecutive_failures > 0 {
                    alerts.push(AggregateAlert::from_report(
                        AlertKind::Recovered,
                        report,
                        cell.consecutive_failures,
                        cell.last_fingerprint.clone(),
                        cell.last_failure_class,
                    ));
                }
                cell.consecutive_failures = 0;
                cell.last_fingerprint = None;
                cell.last_failure_class = None;
            }
            CanaryOutcome::Failed | CanaryOutcome::InfrastructureFailure => {
                let failure = report
                    .failure
                    .as_ref()
                    .expect("validated failed reports contain failure evidence");
                if cell.last_fingerprint.as_deref() == Some(failure.fingerprint.as_str()) {
                    cell.consecutive_failures = cell.consecutive_failures.saturating_add(1);
                } else {
                    cell.consecutive_failures = 1;
                }
                cell.last_fingerprint = Some(failure.fingerprint.clone());
                cell.last_failure_class = Some(failure.class);
                if cell.consecutive_failures <= 2 {
                    alerts.push(AggregateAlert::from_report(
                        if cell.consecutive_failures == 1 {
                            AlertKind::Suspected
                        } else {
                            AlertKind::Confirmed
                        },
                        report,
                        cell.consecutive_failures,
                        cell.last_fingerprint.clone(),
                        cell.last_failure_class,
                    ));
                }
            }
        }
        cell.last_completed_at.clone_from(&report.completed_at);
        cell.last_run_id.clone_from(&report.run_id);
        cell.harness_version.clone_from(&report.harness.version);
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CellState {
    #[serde(default)]
    consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_failure_class: Option<FailureClass>,
    #[serde(default)]
    last_completed_at: String,
    #[serde(default)]
    last_run_id: String,
    #[serde(default)]
    harness_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum AlertKind {
    Suspected,
    Confirmed,
    Recovered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AggregateAlert {
    kind: AlertKind,
    cell: String,
    run_id: String,
    harness: String,
    harness_version: String,
    tier: String,
    scenario: String,
    consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_class: Option<FailureClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<String>,
}

impl AggregateAlert {
    fn from_report(
        kind: AlertKind,
        report: &CanaryReport,
        consecutive_failures: u32,
        fingerprint: Option<String>,
        failure_class: Option<FailureClass>,
    ) -> Self {
        Self {
            kind,
            cell: cell_key(report),
            run_id: report.run_id.clone(),
            harness: report.harness.id.to_string(),
            harness_version: report.harness.version.clone(),
            tier: report.tier.as_str().to_owned(),
            scenario: report.scenario.clone(),
            consecutive_failures,
            failure_class,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AggregateSummary {
    schema_version: u8,
    generated_at: String,
    processed_reports: usize,
    tracked_cells: usize,
    suspected_failures: usize,
    confirmed_failures: usize,
    alerts: Vec<AggregateAlert>,
}

fn cell_key(report: &CanaryReport) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        report.harness.id,
        report.environment.operating_system,
        report.environment.architecture,
        report.tier.as_str(),
        report.scenario
    )
}

fn timestamp() -> Result<String, AggregateError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(AggregateError::Timestamp)
}

fn atomic_json_write(path: &Path, value: &impl Serialize) -> Result<(), AggregateError> {
    let parent = path
        .parent()
        .ok_or_else(|| AggregateError::InvalidOutputPath(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| AggregateError::CreateDirectory {
        path: parent.to_owned(),
        source,
    })?;
    let payload = serde_json::to_vec_pretty(value).map_err(AggregateError::Serialize)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-canary-aggregate-")
        .tempfile_in(parent)
        .map_err(|source| AggregateError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .write_all(&payload)
        .and_then(|()| temporary.write_all(b"\n"))
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| AggregateError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| AggregateError::WriteOutput {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum AggregateError {
    #[error("could not read report directory '{}': {source}", path.display())]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("report directory '{}' contains no JSON reports", .0.display())]
    NoReports(PathBuf),
    #[error(transparent)]
    Report(#[from] ReportError),
    #[error("could not read aggregate state '{}': {source}", path.display())]
    ReadState {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse aggregate state '{}': {source}", path.display())]
    ParseState {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("aggregate state schema {0} is unsupported")]
    UnsupportedStateSchema(u8),
    #[error("aggregate output path '{}' has no parent directory", .0.display())]
    InvalidOutputPath(PathBuf),
    #[error("could not create aggregate directory '{}': {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize aggregate output: {0}")]
    Serialize(serde_json::Error),
    #[error("could not write aggregate output '{}': {source}", path.display())]
    WriteOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not format aggregate timestamp: {0}")]
    Timestamp(time::error::Format),
}

#[cfg(test)]
mod tests {
    use super::{AggregateState, AlertKind};
    use crate::report::{
        CanaryOutcome, CanaryReport, CanaryTier, CanaryTrigger, CheckReport, CheckStatus,
        EnvironmentEvidence, FailureClass, FailureIdentity, FailureReport, HarnessEvidence,
        NanEvidence, REPORT_SCHEMA_VERSION,
    };
    use nan_harness_core::HarnessKind;

    fn report(run: u8, outcome: CanaryOutcome) -> CanaryReport {
        let completed_at = format!("2026-08-22T08:00:0{run}Z");
        let failure = (outcome != CanaryOutcome::Passed).then(|| {
            FailureReport::new(
                FailureClass::Harness,
                "tool",
                None,
                "tool failed",
                &FailureIdentity {
                    harness: HarnessKind::KimiCode,
                    harness_version: "1.2.3",
                    operating_system: "linux",
                    architecture: "aarch64",
                    tier: CanaryTier::LiveExtended,
                    scenario: "edit",
                },
            )
        });
        CanaryReport {
            schema_version: REPORT_SCHEMA_VERSION,
            run_id: format!("run-{run}"),
            cell_id: "linux-kimi-live-edit".to_owned(),
            spec_sha256: "b".repeat(64),
            trigger: CanaryTrigger::Weekly,
            tier: CanaryTier::LiveExtended,
            scenario: "edit".to_owned(),
            started_at: "2026-08-22T08:00:00Z".to_owned(),
            completed_at,
            duration_milliseconds: 1_000,
            nan: NanEvidence {
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
                id: HarnessKind::KimiCode,
                version: "1.2.3".to_owned(),
            },
            model: Some("qwen3.6".to_owned()),
            checks: vec![CheckReport {
                name: "tool-edit".to_owned(),
                status: if outcome == CanaryOutcome::Passed {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                },
                duration_milliseconds: 1_000,
                attempts: 1,
                detail: None,
            }],
            outcome,
            failure,
        }
    }

    #[test]
    fn repeated_failure_is_confirmed_and_recovery_is_emitted() {
        let mut state = AggregateState::default();
        let mut alerts = Vec::new();

        assert!(state.observe(&report(1, CanaryOutcome::Failed), &mut alerts));
        assert_eq!(
            alerts.last().expect("alert should exist").kind,
            AlertKind::Suspected
        );
        assert!(state.observe(&report(2, CanaryOutcome::Failed), &mut alerts));
        assert_eq!(
            alerts.last().expect("alert should exist").kind,
            AlertKind::Confirmed
        );
        assert!(state.observe(&report(3, CanaryOutcome::Passed), &mut alerts));
        assert_eq!(
            alerts.last().expect("alert should exist").kind,
            AlertKind::Recovered
        );
    }

    #[test]
    fn same_report_is_not_counted_twice() {
        let mut state = AggregateState::default();
        let mut alerts = Vec::new();
        let report = report(1, CanaryOutcome::Failed);

        assert!(state.observe(&report, &mut alerts));
        assert!(!state.observe(&report, &mut alerts));
        assert_eq!(alerts.len(), 1);
    }
}
