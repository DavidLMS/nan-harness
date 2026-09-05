mod rendering;
#[cfg(test)]
mod tests;
mod transitions;

use crate::report::FailureClass;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) use rendering::AggregateSummary;

pub(super) const STATE_SCHEMA_VERSION: u8 = 2;
pub(super) const SUMMARY_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AggregateState {
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
    consecutive_inventory_drifts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_inventory_fingerprint: Option<String>,
    #[serde(default)]
    last_completed_at: String,
    #[serde(default)]
    last_run_id: String,
    #[serde(default)]
    harness_version: String,
}

fn cell_key(report: &crate::report::CanaryReport) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        report.harness.id,
        report.environment.operating_system,
        report.environment.architecture,
        report.tier.as_str(),
        report.scenario
    )
}
