use super::super::spec::LoadedSpec;
use super::{CellWorkspace, MAX_CONFORMANCE_REPORT_SIZE};
use nan_harness_core::HarnessKind;
use std::fs;

const SPEC: &str = r#"
schema_version = 1
id = "linux-hermes-daily"
harness = "hermes"
trigger = "daily"
tier = "deterministic"
scenario = "text"
image = "local-image"
guest = "linux"
profile = "node-24"
harness_version_file = "versions/harness.txt"

[nan_harness]
version = "0.0.6"
source = "release"
artifact = "nan-harness"

[[artifacts]]
source = "fixture.txt"
name = "fixture.txt"

[[steps]]
name = "prompt"
script = "true"
failure_class = "harness"
"#;

fn prepared_fixture() -> (tempfile::TempDir, LoadedSpec) {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    fs::write(
        directory.path().join("nan-harness"),
        b"synthetic nan-harness artifact",
    )
    .expect("harness artifact should be written");
    fs::write(
        directory.path().join("fixture.txt"),
        b"synthetic supporting artifact",
    )
    .expect("supporting artifact should be written");
    let spec_path = directory.path().join("cell.toml");
    fs::write(&spec_path, SPEC).expect("cell spec should be written");
    let spec = LoadedSpec::load(&spec_path).expect("fixture spec should load");
    (directory, spec)
}

fn write_report(workspace: &CellWorkspace, contents: &str) {
    fs::write(workspace.output.join("conformance.json"), contents)
        .expect("conformance report should be written");
}

const VALID_REPORT: &str = r#"{
    "schemaVersion": 2,
    "harness": "hermes",
    "scenarios": [{
        "name": "inventory",
        "status": "passed",
        "checks": [{"name": "contract", "status": "passed", "durationMilliseconds": 1}],
        "durationMilliseconds": 1
    }],
    "observations": [{"kind": "inventory-drift", "fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],
    "outcome": "passed",
    "durationMilliseconds": 1
}"#;

#[test]
fn prepare_copies_all_synthetic_artifacts_and_records_digest() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");

    assert_eq!(
        fs::read(workspace.input.join("nan-harness")).expect("copied harness should be readable"),
        b"synthetic nan-harness artifact"
    );
    assert_eq!(
        fs::read(workspace.input.join("fixture.txt")).expect("copied fixture should be readable"),
        b"synthetic supporting artifact"
    );
    assert_eq!(
        workspace.nan_harness_sha256,
        "3dc7f668817c8a44ab7bfee4fa6d7a8ad27a64a3bd31a6efd07f6bffcefa8b28"
    );
}

#[test]
fn log_paths_normalize_separators_and_shell_punctuation() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");

    assert_eq!(
        workspace.log_path("tool/read $result", 2),
        workspace.logs.join("tool-read--result-2.log")
    );
}

#[test]
fn harness_version_extracts_semver_from_output_tokens() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");
    fs::create_dir_all(workspace.output.join("versions")).expect("version directory should exist");
    fs::write(
        workspace.output.join("versions/harness.txt"),
        "hermes version v2.4.0-beta.1+fixture",
    )
    .expect("version output should be written");

    assert_eq!(
        workspace.harness_version(&spec.value),
        Some("2.4.0-beta.1+fixture".to_owned())
    );
}

#[test]
fn valid_conformance_report_returns_typed_observations() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");
    write_report(&workspace, VALID_REPORT);

    let observations = workspace
        .conformance_observations(HarnessKind::Hermes)
        .expect("valid report should be accepted");
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].fingerprint, "a".repeat(64));
}

#[test]
fn malformed_shape_and_harness_mismatch_are_rejected() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");
    write_report(
        &workspace,
        &VALID_REPORT.replace("\"harness\": \"hermes\"", "\"harness\": \"claude-code\""),
    );
    assert!(
        workspace
            .conformance_observations(HarnessKind::Hermes)
            .is_err()
    );

    write_report(&workspace, &VALID_REPORT.replace("\"checks\": [{\"name\": \"contract\", \"status\": \"passed\", \"durationMilliseconds\": 1}]", "\"checks\": []"));
    assert!(
        workspace
            .conformance_observations(HarnessKind::Hermes)
            .is_err()
    );
}

#[test]
fn oversized_conformance_report_is_rejected_before_parsing() {
    let (_directory, spec) = prepared_fixture();
    let workspace = CellWorkspace::prepare(&spec).expect("workspace should prepare");
    // Trailing whitespace keeps the JSON valid, so only the size limit rejects it.
    let mut report = VALID_REPORT.as_bytes().to_vec();
    report.resize(
        usize::try_from(MAX_CONFORMANCE_REPORT_SIZE + 1).unwrap(),
        b' ',
    );
    fs::write(workspace.output.join("conformance.json"), report)
        .expect("oversized report should be written");

    assert!(
        workspace
            .conformance_observations(HarnessKind::Hermes)
            .is_err()
    );
}
