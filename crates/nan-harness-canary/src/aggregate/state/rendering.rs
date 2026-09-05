use super::{AggregateState, SUMMARY_SCHEMA_VERSION, cell_key};
use crate::report::{CanaryReport, FailureClass};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AlertKind {
    Suspected,
    Confirmed,
    Recovered,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AlertSubject {
    Compatibility,
    InventoryDrift,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AggregateAlert {
    pub(super) subject: AlertSubject,
    pub(super) kind: AlertKind,
    pub(super) cell: String,
    pub(super) run_id: String,
    pub(super) harness: String,
    pub(super) harness_version: String,
    pub(super) tier: String,
    pub(super) scenario: String,
    pub(super) consecutive_occurrences: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) failure_class: Option<FailureClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) fingerprint: Option<String>,
}

impl AggregateAlert {
    pub(crate) fn from_report(
        subject: AlertSubject,
        kind: AlertKind,
        report: &CanaryReport,
        consecutive_occurrences: u32,
        fingerprint: Option<String>,
        failure_class: Option<FailureClass>,
    ) -> Self {
        Self {
            subject,
            kind,
            cell: cell_key(report),
            run_id: report.run_id.clone(),
            harness: report.harness.id.to_string(),
            harness_version: report.harness.version.clone(),
            tier: report.tier.as_str().to_owned(),
            scenario: report.scenario.clone(),
            consecutive_occurrences,
            failure_class,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AggregateSummary {
    pub(super) schema_version: u8,
    pub(super) generated_at: String,
    pub(super) processed_reports: usize,
    pub(super) tracked_cells: usize,
    pub(super) suspected_failures: usize,
    pub(super) confirmed_failures: usize,
    pub(super) suspected_inventory_drifts: usize,
    pub(super) confirmed_inventory_drifts: usize,
    pub(super) alerts: Vec<AggregateAlert>,
}

impl AggregateSummary {
    pub(crate) fn new(
        state: &AggregateState,
        processed_reports: usize,
        alerts: Vec<AggregateAlert>,
    ) -> Self {
        Self {
            schema_version: SUMMARY_SCHEMA_VERSION,
            generated_at: state.updated_at.clone(),
            processed_reports,
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
            suspected_inventory_drifts: state
                .cells
                .values()
                .filter(|cell| cell.consecutive_inventory_drifts == 1)
                .count(),
            confirmed_inventory_drifts: state
                .cells
                .values()
                .filter(|cell| cell.consecutive_inventory_drifts >= 2)
                .count(),
            alerts,
        }
    }
}
