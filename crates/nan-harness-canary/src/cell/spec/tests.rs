use super::super::errors::CellError;
use super::LoadedSpec;
use std::fs;
use std::path::{Path, PathBuf};

const VALID_SPEC: &str = r#"
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

[[steps]]
name = "prompt"
script = "test -f result.txt"
failure_class = "harness"
"#;

fn write_spec(directory: &Path, contents: &str) -> PathBuf {
    let path = directory.join("cell.toml");
    fs::write(&path, contents).expect("cell spec should be written");
    path
}

fn load(directory: &Path, contents: &str) -> Result<LoadedSpec, CellError> {
    LoadedSpec::load(&write_spec(directory, contents))
}

fn replace_once(contents: &str, from: &str, to: &str) -> String {
    contents.replacen(from, to, 1)
}

#[test]
fn valid_spec_uses_documented_defaults_and_resolves_relative_paths() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let loaded = load(directory.path(), VALID_SPEC).expect("valid spec should load");

    assert_eq!(loaded.value.boot_timeout_seconds, 180);
    assert_eq!(loaded.value.clone_timeout_seconds, 1_800);
    assert_eq!(loaded.value.overall_timeout_seconds, 1_800);
    assert_eq!(loaded.value.steps[0].timeout_seconds, 300);
    assert_eq!(loaded.value.steps[0].attempts, 1);
    assert_eq!(
        loaded
            .resolve(Path::new("nan-harness"))
            .expect("relative path should resolve"),
        directory.path().join("nan-harness")
    );
    assert_eq!(loaded.sha256.len(), 64);
}

#[test]
fn unsupported_schema_is_rejected_with_typed_error() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let result = load(
        directory.path(),
        &replace_once(VALID_SPEC, "schema_version = 1", "schema_version = 2"),
    );

    assert!(matches!(result, Err(CellError::UnsupportedSpecSchema(2))));
}

#[test]
fn missing_required_field_is_rejected_during_deserialization() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let result = load(
        directory.path(),
        &replace_once(VALID_SPEC, "id = \"linux-hermes-daily\"\n", ""),
    );

    assert!(matches!(result, Err(CellError::ParseSpec { .. })));
}

#[test]
fn empty_required_field_is_rejected_during_validation() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let result = load(
        directory.path(),
        &replace_once(VALID_SPEC, "profile = \"node-24\"", "profile = \"  \""),
    );

    assert!(matches!(result, Err(CellError::EmptySpecField("profile"))));
}

#[test]
fn invalid_nan_harness_version_is_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let result = load(
        directory.path(),
        &replace_once(
            VALID_SPEC,
            "version = \"0.0.6\"",
            "version = \"release-head\"",
        ),
    );

    assert!(matches!(
        result,
        Err(CellError::InvalidNanHarnessVersion(_))
    ));
}

#[test]
fn empty_steps_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let contents = VALID_SPEC
        .split_once("[[steps]]")
        .expect("fixture should contain a step")
        .0
        .replace("\n[nan_harness]", "\nsteps = []\n\n[nan_harness]");
    let result = load(directory.path(), &contents);

    assert!(matches!(result, Err(CellError::MissingSteps)));
}

#[test]
fn zero_timeout_and_attempts_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let timeout = replace_once(
        VALID_SPEC,
        "profile = \"node-24\"",
        "profile = \"node-24\"\noverall_timeout_seconds = 0",
    );
    let attempts = replace_once(
        VALID_SPEC,
        "failure_class = \"harness\"",
        "failure_class = \"harness\"\nattempts = 0",
    );

    assert!(matches!(
        load(directory.path(), &timeout),
        Err(CellError::InvalidTimeout)
    ));
    assert!(matches!(
        load(directory.path(), &attempts),
        Err(CellError::InvalidTimeout)
    ));
}

#[test]
fn unsafe_relative_paths_are_rejected_for_each_path_field() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    for (field, value, error_field) in [
        (
            "artifact = \"nan-harness\"",
            "../nan-harness",
            "nanHarness.artifact",
        ),
        (
            "harness_version_file = \"versions/harness.txt\"",
            "/tmp/version.txt",
            "harnessVersionFile",
        ),
    ] {
        let contents = replace_once(
            VALID_SPEC,
            field,
            &format!("{} = \"{}\"", field.split_once(" = ").unwrap().0, value),
        );
        assert!(matches!(
            load(directory.path(), &contents),
            Err(CellError::UnsafeRelativePath(actual)) if actual == error_field
        ));
    }
}

#[test]
fn artifact_names_reject_nested_paths() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let contents = format!(
        "{VALID_SPEC}\n[[artifacts]]\nsource = \"fixture.txt\"\nname = \"nested/output.txt\"\n"
    );

    assert!(matches!(
        load(directory.path(), &contents),
        Err(CellError::InvalidArtifactName(name)) if name == "nested/output.txt"
    ));
}
