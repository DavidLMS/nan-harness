use super::super::spec::LoadedSpec;
use super::super::workspace::CellWorkspace;
use super::{
    ExecutionTiming, RuntimeFailure, build_report, failed_check, passed_check,
    preserve_private_logs,
};
use crate::report::{
    CanaryObservation, CanaryObservationKind, CanaryOutcome, FailureClass, FailureIdentity,
    FailureReport, RuntimeEvidence, sha256_hex,
};
use nan_harness_core::HarnessKind;
use std::fs;
use std::time::Duration;

const NAN_HARNESS_BYTES: &[u8] = b"synthetic canary nan-harness";
const SPEC: &str = r#"
schema_version = 1
id = "linux-claude-manual"
harness = "claude-code"
trigger = "manual"
tier = "live-core"
scenario = "tool-write"
image = "ubuntu-canary"
guest = "linux"
profile = "node-24"
harness_version_file = "version.txt"
model = "qwen3.6"

[[runtimes]]
name = "node"
version = "24.4.0"

[nan_harness]
version = "0.0.6"
source = "release"
artifact = "nan-harness"

[[steps]]
name = "prompt"
script = "true"
failure_class = "harness"
"#;

fn fixture() -> (tempfile::TempDir, LoadedSpec, CellWorkspace) {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    fs::write(directory.path().join("nan-harness"), NAN_HARNESS_BYTES)
        .expect("harness artifact should be written");
    let spec_path = directory.path().join("cell.toml");
    fs::write(&spec_path, SPEC).expect("cell spec should be written");
    let loaded = LoadedSpec::load(&spec_path).expect("cell spec should load");
    let workspace = CellWorkspace::prepare(&loaded).expect("workspace should prepare");
    (directory, loaded, workspace)
}

fn timing(duration: Duration) -> ExecutionTiming {
    ExecutionTiming {
        started_at: "2026-09-06T08:00:00Z".to_owned(),
        completed_at: "2026-09-06T08:00:03Z".to_owned(),
        duration,
    }
}

fn observation() -> CanaryObservation {
    CanaryObservation {
        kind: CanaryObservationKind::InventoryDrift,
        fingerprint: "f".repeat(64),
    }
}

#[test]
fn successful_build_report_preserves_supplied_metadata_and_check_order() {
    let (_directory, loaded, workspace) = fixture();
    let check = passed_check("tool-write", Duration::from_millis(1_500), 2);
    let report = build_report(
        loaded,
        workspace,
        Some("qwen3.6".to_owned()),
        "2.1.233".to_owned(),
        vec![observation()],
        timing(Duration::from_secs(3)),
        Ok(vec![check]),
    );

    report
        .validate()
        .expect("successful report should validate");
    assert_eq!(report.schema_version, crate::report::REPORT_SCHEMA_VERSION);
    assert!(report.run_id.starts_with("linux-claude-manual-"));
    assert_eq!(report.spec_sha256, sha256_hex(SPEC.as_bytes()));
    assert_eq!(report.nan_harness.sha256, sha256_hex(NAN_HARNESS_BYTES));
    assert_ne!(report.spec_sha256, report.nan_harness.sha256);
    assert_eq!(report.trigger, crate::report::CanaryTrigger::Manual);
    assert_eq!(report.tier, crate::report::CanaryTier::LiveCore);
    assert_eq!(report.scenario, "tool-write");
    assert_eq!(report.started_at, "2026-09-06T08:00:00Z");
    assert_eq!(report.completed_at, "2026-09-06T08:00:03Z");
    assert_eq!(report.duration_milliseconds, 3_000);
    assert_eq!(report.nan_harness.version, "0.0.6");
    assert_eq!(report.nan_harness.source, "release");
    assert_eq!(report.environment.operating_system, "linux");
    assert_eq!(report.environment.architecture, "aarch64");
    assert_eq!(report.environment.image, "ubuntu-canary");
    assert_eq!(report.environment.profile, "node-24");
    assert_eq!(
        report.environment.runtimes,
        vec![RuntimeEvidence {
            name: "node".to_owned(),
            version: "24.4.0".to_owned(),
        }]
    );
    assert_eq!(report.harness.id, HarnessKind::ClaudeCode);
    assert_eq!(report.harness.version, "2.1.233");
    assert_eq!(report.model, Some("qwen3.6".to_owned()));
    assert_eq!(
        report.checks,
        vec![passed_check("tool-write", Duration::from_millis(1_500), 2)]
    );
    assert_eq!(report.observations, vec![observation()]);
    assert_eq!(report.outcome, CanaryOutcome::Passed);
    assert!(report.failure.is_none());
}

#[test]
fn build_report_maps_infrastructure_and_product_failures_to_outcomes() {
    for (class, expected_outcome) in [
        (
            FailureClass::Infrastructure,
            CanaryOutcome::InfrastructureFailure,
        ),
        (FailureClass::Harness, CanaryOutcome::Failed),
        (FailureClass::Provider, CanaryOutcome::Failed),
        (FailureClass::TestContract, CanaryOutcome::Failed),
    ] {
        let (_directory, loaded, workspace) = fixture();
        let execution = Err(RuntimeFailure::new(
            class,
            "tool-write",
            "the step did not complete",
            vec![
                passed_check("prepare", Duration::from_millis(250), 1),
                failed_check(
                    "tool-write",
                    Duration::from_millis(1_500),
                    3,
                    "expected synthetic tool result",
                ),
            ],
        ));
        let report = build_report(
            loaded,
            workspace,
            None,
            "2.1.233".to_owned(),
            Vec::new(),
            timing(Duration::from_millis(1_750)),
            execution,
        );
        let failure = report.failure.as_ref().expect("failure should be present");
        let expected_failure = FailureReport::new(
            class,
            "tool-write",
            None,
            "the step did not complete",
            &FailureIdentity {
                harness: HarnessKind::ClaudeCode,
                harness_version: "2.1.233",
                operating_system: "linux",
                architecture: "aarch64",
                tier: crate::report::CanaryTier::LiveCore,
                scenario: "tool-write",
            },
        );

        assert_eq!(failure, &expected_failure);
        assert_eq!(report.outcome, expected_outcome);
        assert_eq!(report.checks.len(), 2);
        assert_eq!(
            report.checks[0],
            passed_check("prepare", Duration::from_millis(250), 1)
        );
        assert_eq!(
            report.checks[1],
            failed_check(
                "tool-write",
                Duration::from_millis(1_500),
                3,
                "expected synthetic tool result",
            )
        );
    }
}

#[test]
fn timing_duration_saturates_at_the_maximum_millisecond_count() {
    let (_directory, loaded, workspace) = fixture();
    let report = build_report(
        loaded,
        workspace,
        None,
        "2.1.233".to_owned(),
        Vec::new(),
        timing(Duration::MAX),
        Ok(vec![passed_check("prepare", Duration::MAX, 1)]),
    );

    assert_eq!(report.duration_milliseconds, u64::MAX);
    assert_eq!(report.checks[0].duration_milliseconds, u64::MAX);
}

#[test]
fn missing_private_log_directory_leaves_success_and_failure_unchanged() {
    let (_directory, _loaded, workspace) = fixture();
    let checks = vec![passed_check("tool-write", Duration::from_secs(1), 1)];
    let preserved_success = preserve_private_logs(&workspace, None, Ok(checks.clone()));
    let preserved_failure = preserve_private_logs(
        &workspace,
        None,
        Err(RuntimeFailure::new(
            FailureClass::Provider,
            "provider-call",
            "the provider was unavailable",
            checks,
        )),
    );

    let Ok(success) = preserved_success else {
        panic!("success should remain successful");
    };
    assert_eq!(
        success,
        vec![passed_check("tool-write", Duration::from_secs(1), 1)]
    );
    let Err(failure) = preserved_failure else {
        panic!("failure should remain a failure");
    };
    assert_eq!(failure.class, FailureClass::Provider);
    assert_eq!(failure.phase, "provider-call");
    assert_eq!(failure.summary, "the provider was unavailable");
    assert_eq!(
        failure.checks,
        vec![passed_check("tool-write", Duration::from_secs(1), 1)]
    );
}

#[test]
fn successful_preservation_copies_private_logs_and_keeps_success() {
    let (_directory, loaded, workspace) = fixture();
    let destination = tempfile::tempdir().expect("destination root should exist");
    let destination_path = destination.path().join("preserved");
    let source_log = workspace.log_path("tool-write", 1);
    fs::write(&source_log, b"synthetic private log").expect("source log should be written");

    let preserved = preserve_private_logs(
        &workspace,
        Some(&destination_path),
        Ok(vec![passed_check("tool-write", Duration::from_secs(1), 1)]),
    );
    let Ok(checks) = preserved else {
        panic!("preservation should succeed");
    };
    assert_eq!(
        checks,
        vec![passed_check("tool-write", Duration::from_secs(1), 1)]
    );
    assert_eq!(
        fs::read(destination_path.join("tool-write-1.log")).expect("copied log should be readable"),
        b"synthetic private log"
    );
    assert_eq!(
        fs::read(&source_log).expect("source log should remain readable"),
        b"synthetic private log"
    );

    let report = build_report(
        loaded,
        workspace,
        Some("qwen3.6".to_owned()),
        "2.1.233".to_owned(),
        Vec::new(),
        timing(Duration::from_secs(1)),
        Ok(checks),
    );
    assert_eq!(report.outcome, CanaryOutcome::Passed);
    assert!(report.failure.is_none());
}

#[test]
fn preservation_obstruction_fails_success_and_preserves_logs_and_obstruction() {
    let (_directory, loaded, workspace) = fixture();
    let obstruction = tempfile::tempdir().expect("destination root should exist");
    let destination_path = obstruction.path().join("preserved");
    fs::write(&destination_path, b"synthetic obstruction")
        .expect("obstructing file should be written");
    let source_log = workspace.log_path("tool-write", 1);
    fs::write(&source_log, b"synthetic private log").expect("source log should be written");

    let preserved = preserve_private_logs(
        &workspace,
        Some(&destination_path),
        Ok(vec![passed_check("tool-write", Duration::from_secs(1), 1)]),
    );
    let Err(failure) = preserved else {
        panic!("obstructed preservation should fail");
    };
    assert_eq!(failure.class, FailureClass::Infrastructure);
    assert_eq!(failure.phase, "preserve-private-logs");
    assert_eq!(
        failure.summary,
        "private diagnostic logs could not be preserved"
    );
    assert_eq!(failure.checks.len(), 2);
    assert_eq!(
        failure.checks.last(),
        Some(&failed_check(
            "preserve-private-logs",
            Duration::ZERO,
            1,
            "private diagnostic logs could not be preserved",
        ))
    );
    assert_eq!(
        fs::read(&source_log).expect("source log should remain readable"),
        b"synthetic private log"
    );
    assert_eq!(
        fs::read(&destination_path).expect("obstruction should remain readable"),
        b"synthetic obstruction"
    );

    let report = build_report(
        loaded,
        workspace,
        None,
        "2.1.233".to_owned(),
        Vec::new(),
        timing(Duration::from_secs(1)),
        Err(failure),
    );
    assert_eq!(report.outcome, CanaryOutcome::InfrastructureFailure);
    assert_eq!(
        report.failure.as_ref().map(|failure| failure.class),
        Some(FailureClass::Infrastructure)
    );
}

#[test]
fn preservation_obstruction_appends_one_check_to_a_product_failure() {
    let (_directory, loaded, workspace) = fixture();
    let obstruction = tempfile::tempdir().expect("destination root should exist");
    let destination_path = obstruction.path().join("preserved");
    fs::write(&destination_path, b"synthetic obstruction")
        .expect("obstructing file should be written");
    let source_log = workspace.log_path("tool-write", 1);
    fs::write(&source_log, b"synthetic private log").expect("source log should be written");
    let original_checks = vec![
        passed_check("prepare", Duration::from_millis(250), 1),
        failed_check(
            "tool-write",
            Duration::from_millis(1_500),
            2,
            "expected synthetic tool result",
        ),
    ];

    let preserved = preserve_private_logs(
        &workspace,
        Some(&destination_path),
        Err(RuntimeFailure::new(
            FailureClass::Harness,
            "tool-write",
            "the step did not complete",
            original_checks,
        )),
    );
    let Err(failure) = preserved else {
        panic!("obstructed preservation should fail");
    };
    assert_eq!(failure.class, FailureClass::Harness);
    assert_eq!(failure.phase, "tool-write");
    assert_eq!(failure.summary, "the step did not complete");
    assert_eq!(failure.checks.len(), 3);
    assert_eq!(
        failure.checks[0],
        passed_check("prepare", Duration::from_millis(250), 1)
    );
    assert_eq!(
        failure.checks[1],
        failed_check(
            "tool-write",
            Duration::from_millis(1_500),
            2,
            "expected synthetic tool result",
        )
    );
    assert_eq!(
        failure.checks[2],
        failed_check(
            "preserve-private-logs",
            Duration::ZERO,
            1,
            "private diagnostic logs could not be preserved",
        )
    );
    assert_eq!(
        fs::read(&source_log).expect("source log should remain readable"),
        b"synthetic private log"
    );
    assert_eq!(
        fs::read(&destination_path).expect("obstruction should remain readable"),
        b"synthetic obstruction"
    );

    let report = build_report(
        loaded,
        workspace,
        None,
        "2.1.233".to_owned(),
        Vec::new(),
        timing(Duration::from_millis(1_750)),
        Err(failure),
    );
    assert_eq!(report.outcome, CanaryOutcome::Failed);
    let failure = report.failure.as_ref().expect("failure should be present");
    assert_eq!(failure.class, FailureClass::Harness);
    assert_eq!(failure.phase, "tool-write");
    assert_eq!(failure.summary, "the step did not complete");
    assert_eq!(report.checks.len(), 3);
    assert_eq!(
        report.checks.last(),
        Some(&failed_check(
            "preserve-private-logs",
            Duration::ZERO,
            1,
            "private diagnostic logs could not be preserved",
        ))
    );
}
