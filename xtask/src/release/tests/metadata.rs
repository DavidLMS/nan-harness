use super::super::artifacts::{AUXILIARY_ARTIFACTS, RELEASE_TARGETS, artifact_file_name};
use super::super::compatibility::COMPATIBILITY_FILE_NAME;
use super::super::validation::CITATION_FILE_NAME;
use crate::release::{generate_metadata, validate_tag};
use serde_json::Value;
use std::fs;

#[test]
fn accepts_only_the_exact_workspace_release_tag() {
    assert!(validate_tag(&format!("v{}", env!("CARGO_PKG_VERSION"))).is_ok());
    assert!(validate_tag(env!("CARGO_PKG_VERSION")).is_err());
    assert!(validate_tag("v999.0.0").is_err());
}

#[test]
fn creates_the_complete_release_contract() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    fs::write(directory.path().join("install.sh"), "installer")
        .expect("shell installer should exist");
    fs::write(directory.path().join("install.ps1"), "installer")
        .expect("PowerShell installer should exist");
    for target in RELEASE_TARGETS {
        fs::write(directory.path().join(artifact_file_name(target)), target)
            .expect("artifact should exist");
    }
    for artifact in AUXILIARY_ARTIFACTS {
        fs::write(directory.path().join(artifact), artifact)
            .expect("auxiliary artifact should exist");
    }

    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    generate_metadata(&tag, "DavidLMS/nan-harness", directory.path())
        .expect("metadata should be generated");

    let manifest: Value = serde_json::from_slice(
        &fs::read(directory.path().join("update-manifest.json")).expect("manifest should exist"),
    )
    .expect("manifest should be valid JSON");
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        manifest["artifacts"]
            .as_array()
            .expect("artifacts should be an array")
            .len(),
        RELEASE_TARGETS.len()
    );
    assert!(
        manifest["artifacts"]
            .as_array()
            .expect("artifacts should be an array")
            .iter()
            .all(|artifact| artifact["url"]
                .as_str()
                .is_some_and(|url| !url.contains("canary")))
    );
    let compatibility: Value = serde_json::from_slice(
        &fs::read(directory.path().join(COMPATIBILITY_FILE_NAME))
            .expect("compatibility manifest should exist"),
    )
    .expect("compatibility manifest should be valid JSON");
    assert_eq!(compatibility["schemaVersion"], 2);
    assert_eq!(
        compatibility["releases"][0]["verifications"][0]["compatibleAt"],
        "2026-08-29T00:00:00Z"
    );
    assert_eq!(
        compatibility["releases"][0]["verifications"]
            .as_array()
            .expect("verifications should be an array")
            .len(),
        15
    );

    let citation = fs::read_to_string(directory.path().join(CITATION_FILE_NAME))
        .expect("citation file should be generated");
    assert!(citation.contains(&format!("version: \"{}\"", env!("CARGO_PKG_VERSION"))));

    let checksums =
        fs::read_to_string(directory.path().join("SHA256SUMS")).expect("checksums should exist");
    assert!(checksums.contains("  install.sh\n"));
    assert!(checksums.contains("  CITATION.cff\n"));
    assert!(checksums.contains("  compatibility.json\n"));
    assert!(checksums.contains("  LICENSE\n"));
    assert!(checksums.contains("  NOTICE.md\n"));
    assert!(checksums.contains("  update-manifest.json\n"));
    for artifact in AUXILIARY_ARTIFACTS {
        assert!(checksums.contains(&format!("  {artifact}\n")));
    }
}
