use super::rendering::{AggregateAlert, AlertKind, AlertSubject};
use super::{AggregateState, CellState, STATE_SCHEMA_VERSION, cell_key};
use crate::aggregate::errors::AggregateError;
use crate::aggregate::persistence::atomic_json_write;
use crate::report::{CanaryObservationKind, CanaryOutcome, CanaryReport};
use std::fs;
use std::path::Path;

const LEGACY_STATE_SCHEMA_VERSION: u8 = 1;

impl AggregateState {
    pub(crate) fn read_or_default(path: &Path) -> Result<Self, AggregateError> {
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
        let mut state: Self =
            serde_json::from_slice(&contents).map_err(|source| AggregateError::ParseState {
                path: path.to_owned(),
                source,
            })?;
        if !matches!(
            state.schema_version,
            LEGACY_STATE_SCHEMA_VERSION | STATE_SCHEMA_VERSION
        ) {
            return Err(AggregateError::UnsupportedStateSchema(state.schema_version));
        }
        state.schema_version = STATE_SCHEMA_VERSION;
        Ok(state)
    }

    pub(crate) fn write(&self, path: &Path) -> Result<(), AggregateError> {
        atomic_json_write(path, self)
    }

    pub(crate) fn set_updated_at(&mut self, updated_at: String) {
        self.updated_at = updated_at;
    }

    pub(crate) fn observe(
        &mut self,
        report: &CanaryReport,
        alerts: &mut Vec<AggregateAlert>,
    ) -> bool {
        let key = cell_key(report);
        let cell = self.cells.entry(key).or_default();
        if !cell.last_completed_at.is_empty() && report.completed_at <= cell.last_completed_at {
            return false;
        }

        match report.outcome {
            CanaryOutcome::Passed => {
                if cell.consecutive_failures > 0 {
                    alerts.push(AggregateAlert::from_report(
                        AlertSubject::Compatibility,
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
                        AlertSubject::Compatibility,
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
        if report.outcome == CanaryOutcome::Passed {
            cell.observe_inventory(report, alerts);
        }
        cell.last_completed_at.clone_from(&report.completed_at);
        cell.last_run_id.clone_from(&report.run_id);
        cell.harness_version.clone_from(&report.harness.version);
        true
    }
}

impl CellState {
    fn observe_inventory(&mut self, report: &CanaryReport, alerts: &mut Vec<AggregateAlert>) {
        let observation = report
            .observations
            .iter()
            .find(|observation| observation.kind == CanaryObservationKind::InventoryDrift);
        if let Some(observation) = observation {
            if self.last_inventory_fingerprint.as_deref() == Some(observation.fingerprint.as_str())
            {
                self.consecutive_inventory_drifts =
                    self.consecutive_inventory_drifts.saturating_add(1);
            } else {
                self.consecutive_inventory_drifts = 1;
            }
            self.last_inventory_fingerprint = Some(observation.fingerprint.clone());
            if self.consecutive_inventory_drifts <= 2 {
                alerts.push(AggregateAlert::from_report(
                    AlertSubject::InventoryDrift,
                    if self.consecutive_inventory_drifts == 1 {
                        AlertKind::Suspected
                    } else {
                        AlertKind::Confirmed
                    },
                    report,
                    self.consecutive_inventory_drifts,
                    self.last_inventory_fingerprint.clone(),
                    None,
                ));
            }
        } else {
            if self.consecutive_inventory_drifts > 0 {
                alerts.push(AggregateAlert::from_report(
                    AlertSubject::InventoryDrift,
                    AlertKind::Recovered,
                    report,
                    self.consecutive_inventory_drifts,
                    self.last_inventory_fingerprint.clone(),
                    None,
                ));
            }
            self.consecutive_inventory_drifts = 0;
            self.last_inventory_fingerprint = None;
        }
    }
}
