#![cfg(unix)]

use nan_harness_core::{HarnessKind, VersionStatus};
use nan_harness_runtime::{
    DiscoveryError, DiscoveryOptions, bundled_compatibility_manifest, discover_harness,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
fn bundled_manifest_is_typed_and_complete() {
    let manifest = bundled_compatibility_manifest().expect("manifest should parse");

    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.harnesses.len(), 5);
    assert!(manifest.entry(HarnessKind::ClaudeCode).is_some());
}

#[test]
fn discovery_classifies_tested_newer_and_overridden_versions() {
    let tested = fake_executable("claude 2.1.233");
    let report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&tested),
        DiscoveryOptions::default(),
    )
    .expect("tested version should pass");
    assert_eq!(report.harness.version_status, VersionStatus::Tested);
    assert!(report.warnings.is_empty());

    let newer = fake_executable("claude 2.2.0");
    let report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&newer),
        DiscoveryOptions::default(),
    )
    .expect("newer version should pass with warning");
    assert_eq!(report.harness.version_status, VersionStatus::NewerUntested);
    assert_eq!(report.warnings.len(), 1);

    let older = fake_executable("claude 2.0.0");
    let rejected = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&older),
        DiscoveryOptions::default(),
    );
    assert!(matches!(
        rejected,
        Err(DiscoveryError::UnsupportedVersion { .. })
    ));
    let allowed = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&older),
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: false,
        },
    )
    .expect("explicit override should permit old version");
    assert_eq!(
        allowed.harness.version_status,
        VersionStatus::OlderUnsupported
    );
}

#[test]
fn unparseable_versions_require_an_explicit_override() {
    let executable = fake_executable("development build");
    let rejected = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&executable),
        DiscoveryOptions::default(),
    );
    assert!(matches!(
        rejected,
        Err(DiscoveryError::UnparseableVersion { .. })
    ));

    let allowed = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&executable),
        DiscoveryOptions {
            allow_unsupported: false,
            allow_untested: true,
        },
    )
    .expect("explicit override should permit unparseable version");
    assert_eq!(allowed.harness.version_status, VersionStatus::Unparseable);
}

fn fake_executable(version_output: &str) -> PathBuf {
    let directory = tempfile::tempdir()
        .expect("temporary directory should be created")
        .keep();
    let executable = directory.join("fake-harness");
    fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{version_output}'\n"),
    )
    .expect("fake executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}
