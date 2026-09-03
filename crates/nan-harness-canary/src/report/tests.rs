use super::{
    CanaryObservation, CanaryObservationKind, CanaryOutcome, CanaryReport, CanaryTier,
    CanaryTrigger, CheckReport, CheckStatus, EnvironmentEvidence, FailureClass, FailureIdentity,
    FailureReport, HarnessEvidence, NanHarnessEvidence, REPORT_SCHEMA_VERSION, sha256_hex,
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
        observations: Vec::new(),
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
fn passed_reports_accept_prerelease_and_build_metadata() {
    let mut report = report();
    report.harness.version = "1.2.3-rc.1+build.7".to_owned();

    report
        .validate()
        .expect("prerelease and build metadata should be valid semantic versioning");
}

#[test]
fn legacy_reports_remain_readable_without_observations() {
    let mut value = serde_json::to_value(report()).expect("report should serialize");
    value["schemaVersion"] = serde_json::json!(1);
    value["environment"]
        .as_object_mut()
        .expect("environment should be an object")
        .remove("runtimes");

    let mut report: CanaryReport =
        serde_json::from_value(value).expect("legacy report should deserialize");
    assert!(report.environment.runtimes.is_empty());
    assert!(report.observations.is_empty());
    report
        .validate()
        .expect("schema-v1 reports should remain readable");

    report.observations.push(CanaryObservation {
        kind: CanaryObservationKind::InventoryDrift,
        fingerprint: "c".repeat(64),
    });
    assert!(matches!(
        report.validate(),
        Err(super::ReportError::LegacyObservations)
    ));
}

#[test]
fn inventory_drift_observation_is_bounded_and_safe() {
    let mut report = report();
    report.observations.push(CanaryObservation {
        kind: CanaryObservationKind::InventoryDrift,
        fingerprint: "c".repeat(64),
    });
    report.validate().expect("observation should be valid");
    let encoded = serde_json::to_string(&report).expect("report should serialize");
    assert!(encoded.contains("inventory-drift"));
    assert!(!encoded.contains("read_file"));
    assert!(!encoded.contains("write_file"));
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
    assert_eq!(
        first.fingerprint,
        "0d60331fb4f54349ab29b8961f7a39dcfe316e970185ad2d320424ccafaf1390"
    );
}

#[test]
fn sha256_hex_matches_known_digest() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn serialized_report_preserves_wire_contract() {
    assert_eq!(
        serde_json::to_value(report()).expect("report should serialize"),
        serde_json::json!({
            "schemaVersion": 2,
            "runId": "run-2026-08-22",
            "cellId": "linux-claude-live-read",
            "specSha256": "b".repeat(64),
            "trigger": "daily",
            "tier": "live-core",
            "scenario": "read",
            "startedAt": "2026-08-22T08:00:00Z",
            "completedAt": "2026-08-22T08:00:03Z",
            "durationMilliseconds": 3_000,
            "nanHarness": {
                "version": "0.0.6",
                "source": "release",
                "sha256": "a".repeat(64),
            },
            "environment": {
                "operatingSystem": "linux",
                "architecture": "aarch64",
                "image": "ubuntu",
                "profile": "node-24",
                "runtimes": [],
            },
            "harness": {
                "id": "claude-code",
                "version": "2.1.233",
            },
            "model": "qwen3.6",
            "checks": [{
                "name": "tool-read",
                "status": "passed",
                "durationMilliseconds": 1_000,
                "attempts": 1,
            }],
            "outcome": "passed",
        })
    );
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
