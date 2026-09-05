use super::rendering::{AlertKind, AlertSubject};
use super::{AggregateState, AggregateSummary, STATE_SCHEMA_VERSION, SUMMARY_SCHEMA_VERSION};
use crate::report::{
    CanaryObservation, CanaryObservationKind, CanaryOutcome, CanaryReport, CanaryTier,
    CanaryTrigger, CheckReport, CheckStatus, EnvironmentEvidence, FailureClass, FailureIdentity,
    FailureReport, HarnessEvidence, NanHarnessEvidence, REPORT_SCHEMA_VERSION,
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
        observations: Vec::new(),
        outcome,
        failure,
    }
}

fn drift_report(run: u8, fingerprint: char) -> CanaryReport {
    let mut report = report(run, CanaryOutcome::Passed);
    report.observations.push(CanaryObservation {
        kind: CanaryObservationKind::InventoryDrift,
        fingerprint: fingerprint.to_string().repeat(64),
    });
    report
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

#[test]
fn inventory_drift_is_confirmed_separately_and_recovers() {
    let mut state = AggregateState::default();
    let mut alerts = Vec::new();

    assert!(state.observe(&drift_report(1, 'c'), &mut alerts));
    assert_eq!(
        alerts.last().expect("alert should exist").subject,
        AlertSubject::InventoryDrift
    );
    assert_eq!(
        alerts.last().expect("alert should exist").kind,
        AlertKind::Suspected
    );
    assert!(state.observe(&drift_report(2, 'c'), &mut alerts));
    assert_eq!(
        alerts.last().expect("alert should exist").kind,
        AlertKind::Confirmed
    );
    let alert_count = alerts.len();
    assert!(state.observe(&drift_report(3, 'c'), &mut alerts));
    assert_eq!(alerts.len(), alert_count);
    assert!(state.observe(&report(4, CanaryOutcome::Passed), &mut alerts));
    assert_eq!(
        alerts.last().expect("alert should exist").kind,
        AlertKind::Recovered
    );
    assert_eq!(
        alerts.last().expect("alert should exist").subject,
        AlertSubject::InventoryDrift
    );
}

#[test]
fn changed_inventory_fingerprint_restarts_confirmation() {
    let mut state = AggregateState::default();
    let mut alerts = Vec::new();

    assert!(state.observe(&drift_report(1, 'c'), &mut alerts));
    assert!(state.observe(&drift_report(2, 'd'), &mut alerts));
    assert_eq!(
        alerts.last().expect("alert should exist").kind,
        AlertKind::Suspected
    );
    assert_eq!(
        alerts
            .last()
            .expect("alert should exist")
            .consecutive_occurrences,
        1
    );
}

#[test]
fn compatibility_failure_does_not_resolve_inventory_drift() {
    let mut state = AggregateState::default();
    let mut alerts = Vec::new();

    assert!(state.observe(&drift_report(1, 'c'), &mut alerts));
    assert!(state.observe(&report(2, CanaryOutcome::Failed), &mut alerts));
    assert!(state.observe(&drift_report(3, 'c'), &mut alerts));
    let drift = alerts
        .iter()
        .rev()
        .find(|alert| alert.subject == AlertSubject::InventoryDrift)
        .expect("inventory alert should exist");
    assert_eq!(drift.kind, AlertKind::Confirmed);
}

#[test]
fn summary_preserves_counts_and_json_field_names() {
    let mut state = AggregateState::default();
    state.set_updated_at("2026-08-22T08:00:03Z".to_owned());
    let mut alerts = Vec::new();
    assert!(state.observe(&report(1, CanaryOutcome::Failed), &mut alerts));
    let mut inventory = drift_report(2, 'c');
    inventory.scenario = "inventory".to_owned();
    assert!(state.observe(&inventory, &mut alerts));

    let summary = AggregateSummary::new(&state, 2, alerts);

    assert_eq!(summary.schema_version, SUMMARY_SCHEMA_VERSION);
    assert_eq!(summary.generated_at, "2026-08-22T08:00:03Z");
    assert_eq!(summary.processed_reports, 2);
    assert_eq!(summary.tracked_cells, 2);
    assert_eq!(summary.suspected_failures, 1);
    assert_eq!(summary.confirmed_failures, 0);
    assert_eq!(summary.suspected_inventory_drifts, 1);
    assert_eq!(summary.confirmed_inventory_drifts, 0);
    let value = serde_json::to_value(summary).expect("summary should serialize");
    assert_eq!(value["schemaVersion"], SUMMARY_SCHEMA_VERSION);
    assert_eq!(value["processedReports"], 2);
    assert!(value.get("schema_version").is_none());
    assert_eq!(value["alerts"][0]["subject"], "compatibility");
    assert_eq!(value["alerts"][1]["subject"], "inventory-drift");
}

#[test]
fn legacy_aggregate_state_migrates_without_losing_failures() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let path = directory.path().join("aggregate-state.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "updatedAt": "2026-08-22T08:00:00Z",
            "cells": {
                "legacy": {
                    "consecutiveFailures": 2,
                    "lastFingerprint": "a".repeat(64),
                    "lastFailureClass": "harness",
                    "lastCompletedAt": "2026-08-22T08:00:00Z",
                    "lastRunId": "run-1",
                    "harnessVersion": "1.2.3"
                }
            }
        }))
        .expect("state should serialize"),
    )
    .expect("state should be written");

    let state = AggregateState::read_or_default(&path).expect("state should migrate");
    assert_eq!(state.schema_version, STATE_SCHEMA_VERSION);
    assert_eq!(state.cells["legacy"].consecutive_failures, 2);
    assert_eq!(state.cells["legacy"].consecutive_inventory_drifts, 0);
}
