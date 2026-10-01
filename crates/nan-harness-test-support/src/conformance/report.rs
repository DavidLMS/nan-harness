use super::constants::{
    CONFORMANCE_SCHEMA_VERSION, LEGACY_CONFORMANCE_SCHEMA_VERSION, MAX_DURATION_MILLISECONDS,
    MAX_REPORT_CHECKS, MAX_REPORT_NAME_BYTES, MAX_REPORT_OBSERVATIONS, MAX_REPORT_SCENARIOS,
    PUBLISHED_SCENARIO_NAMES,
};
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceObservationKind {
    InventoryDrift,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum InventoryFailureReason {
    ProcessFailed,
    MarkerMissing,
    ProviderFailed,
    ProviderShutdownFailed,
    DaemonCleanupFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InventoryProcessStatus {
    Completed,
    NonzeroExit,
    LaunchError,
    EnvironmentError,
    Timeout,
    MissingOutput,
    CaptureError,
    CleanupError,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InventoryCleanupStage {
    Terminate,
    Wait,
    WaitTimeout,
    CaptureTimeout,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InventoryCleanupStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryProcessEvidence {
    pub status: InventoryProcessStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_error_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_milliseconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_stage: Option<InventoryCleanupStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_stream: Option<InventoryCleanupStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceObservation {
    pub kind: ConformanceObservationKind,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceCheck {
    pub name: String,
    pub status: ConformanceStatus,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceScenario {
    pub name: String,
    pub status: ConformanceStatus,
    pub checks: Vec<ConformanceCheck>,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceReport {
    pub schema_version: u8,
    pub harness: HarnessKind,
    pub scenarios: Vec<ConformanceScenario>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<ConformanceObservation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory_failure_reasons: Vec<InventoryFailureReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory_process: Option<InventoryProcessEvidence>,
    pub outcome: ConformanceOutcome,
    pub duration_milliseconds: u64,
}

impl ConformanceReport {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.outcome == ConformanceOutcome::Passed
    }

    /// Validates the bounded public report shape before serialization.
    ///
    /// # Errors
    ///
    /// Returns [`ReportShapeError`] when a report contains an unbounded or unknown scenario.
    pub fn validate_shape(&self) -> Result<(), ReportShapeError> {
        if !matches!(
            self.schema_version,
            LEGACY_CONFORMANCE_SCHEMA_VERSION | CONFORMANCE_SCHEMA_VERSION
        ) {
            return Err(ReportShapeError::Schema(self.schema_version));
        }
        if self.schema_version == LEGACY_CONFORMANCE_SCHEMA_VERSION && !self.observations.is_empty()
        {
            return Err(ReportShapeError::LegacyObservations);
        }
        if self.schema_version == LEGACY_CONFORMANCE_SCHEMA_VERSION
            && !self.inventory_failure_reasons.is_empty()
        {
            return Err(ReportShapeError::LegacyInventoryFailureReasons);
        }
        if self.schema_version == LEGACY_CONFORMANCE_SCHEMA_VERSION
            && self.inventory_process.is_some()
        {
            return Err(ReportShapeError::LegacyInventoryProcess);
        }
        if self.inventory_failure_reasons.len() > 5 {
            return Err(ReportShapeError::TooManyInventoryFailureReasons);
        }
        if let Some(process) = &self.inventory_process {
            process
                .validate()
                .map_err(|()| ReportShapeError::InventoryProcess)?;
        }
        if !self.inventory_failure_reasons.is_empty()
            && !self.scenarios.iter().any(|scenario| {
                scenario.name == "inventory" && scenario.status == ConformanceStatus::Failed
            })
        {
            return Err(ReportShapeError::InventoryFailureReasonsWithoutFailure);
        }
        if self
            .inventory_failure_reasons
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != self.inventory_failure_reasons.len()
        {
            return Err(ReportShapeError::DuplicateInventoryFailureReason);
        }
        if self.observations.len() > MAX_REPORT_OBSERVATIONS {
            return Err(ReportShapeError::TooManyObservations(
                self.observations.len(),
            ));
        }
        if self
            .observations
            .iter()
            .any(|observation| !valid_sha256(&observation.fingerprint))
        {
            return Err(ReportShapeError::ObservationFingerprint);
        }
        if self.scenarios.len() > MAX_REPORT_SCENARIOS {
            return Err(ReportShapeError::TooManyScenarios(self.scenarios.len()));
        }
        if self.duration_milliseconds > MAX_DURATION_MILLISECONDS {
            return Err(ReportShapeError::Duration(self.duration_milliseconds));
        }
        for scenario in &self.scenarios {
            validate_report_name(&scenario.name)?;
            if scenario.checks.is_empty() || scenario.checks.len() > MAX_REPORT_CHECKS {
                return Err(ReportShapeError::Checks(scenario.name.clone()));
            }
            if scenario.duration_milliseconds > MAX_DURATION_MILLISECONDS {
                return Err(ReportShapeError::Duration(scenario.duration_milliseconds));
            }
            for check in &scenario.checks {
                validate_report_name(&check.name)?;
                if check.duration_milliseconds > MAX_DURATION_MILLISECONDS {
                    return Err(ReportShapeError::Duration(check.duration_milliseconds));
                }
            }
        }
        Ok(())
    }
}

fn validate_report_name(name: &str) -> Result<(), ReportShapeError> {
    if name.is_empty() || name.len() > MAX_REPORT_NAME_BYTES {
        Err(ReportShapeError::Name)
    } else {
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn validate_published_scenario_set(
    scenarios: &[ConformanceScenario],
) -> Result<(), ReportShapeError> {
    let names = scenarios
        .iter()
        .map(|scenario| scenario.name.as_str())
        .collect::<BTreeSet<_>>();
    if names.len() != PUBLISHED_SCENARIO_NAMES.len()
        || PUBLISHED_SCENARIO_NAMES
            .iter()
            .any(|name| !names.contains(name))
    {
        return Err(ReportShapeError::ScenarioSet);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReportShapeError {
    #[error("unsupported conformance report schema version {0}")]
    Schema(u8),
    #[error("legacy conformance reports cannot contain observations")]
    LegacyObservations,
    #[error("legacy conformance reports cannot contain inventory failure reasons")]
    LegacyInventoryFailureReasons,
    #[error("legacy conformance reports cannot contain inventory process evidence")]
    LegacyInventoryProcess,
    #[error("conformance report contains too many inventory failure reasons")]
    TooManyInventoryFailureReasons,
    #[error("conformance report contains duplicate inventory failure reasons")]
    DuplicateInventoryFailureReason,
    #[error("inventory process evidence is inconsistent")]
    InventoryProcess,
    #[error("inventory failure reasons require a failed inventory scenario")]
    InventoryFailureReasonsWithoutFailure,
    #[error("conformance report contains too many observations: {0}")]
    TooManyObservations(usize),
    #[error("conformance report contains an invalid observation fingerprint")]
    ObservationFingerprint,
    #[error("conformance report contains too many scenarios: {0}")]
    TooManyScenarios(usize),
    #[error("conformance report contains an invalid duration: {0}")]
    Duration(u64),
    #[error("conformance report contains an invalid name")]
    Name,
    #[error("conformance scenario '{0}' has an invalid check list")]
    Checks(String),
    #[error("published conformance report is missing a required scenario")]
    ScenarioSet,
}

impl InventoryProcessEvidence {
    fn validate(&self) -> Result<(), ()> {
        let no_exit = self.exit_code.is_none();
        let no_os_code = self.os_error_code.is_none();
        let no_timeout = self.timeout_milliseconds.is_none();
        let valid = match self.status {
            InventoryProcessStatus::Completed => {
                self.exit_code == Some(0)
                    && no_os_code
                    && no_timeout
                    && self.cleanup_stage.is_none()
                    && self.cleanup_stream.is_none()
            }
            InventoryProcessStatus::NonzeroExit => {
                self.exit_code.is_none_or(|code| code != 0)
                    && no_os_code
                    && no_timeout
                    && self.cleanup_stage.is_none()
                    && self.cleanup_stream.is_none()
            }
            InventoryProcessStatus::LaunchError | InventoryProcessStatus::EnvironmentError => {
                no_exit
                    && no_timeout
                    && self.cleanup_stage.is_none()
                    && self.cleanup_stream.is_none()
            }
            InventoryProcessStatus::Timeout => {
                no_exit
                    && no_os_code
                    && self.cleanup_stage.is_none()
                    && self.cleanup_stream.is_none()
                    && self.timeout_milliseconds.is_some_and(|milliseconds| {
                        milliseconds > 0 && milliseconds <= MAX_DURATION_MILLISECONDS
                    })
            }
            InventoryProcessStatus::MissingOutput | InventoryProcessStatus::CaptureError => {
                no_exit
                    && no_os_code
                    && no_timeout
                    && self.cleanup_stage.is_none()
                    && self.cleanup_stream.is_none()
            }
            InventoryProcessStatus::CleanupError => {
                no_exit
                    && no_timeout
                    && self.cleanup_stage.is_some()
                    && self.cleanup_stream.is_some()
            }
        };
        valid.then_some(()).ok_or(())
    }
}
