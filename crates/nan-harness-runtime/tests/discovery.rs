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

    assert_eq!(manifest.schema_version, 2);
    assert_eq!(manifest.harnesses.len(), 14);
    let claude = manifest
        .entry(HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility should exist");
    assert_eq!(claude.minimum_version.to_string(), "2.1.233");
    assert_eq!(claude.last_verified_version.to_string(), "2.1.233");
    assert!(manifest.entry(HarnessKind::PrimeAgent).is_some());
    assert!(manifest.entry(HarnessKind::DeepSeekHarness).is_some());
    assert!(manifest.entry(HarnessKind::OpenClaw).is_some());
    assert!(manifest.entry(HarnessKind::Cline).is_some());
    assert!(manifest.entry(HarnessKind::QwenCode).is_some());
    assert!(manifest.entry(HarnessKind::KimiCode).is_some());
    assert!(manifest.entry(HarnessKind::Aider).is_some());
    assert!(manifest.entry(HarnessKind::Goose).is_some());
    let fx = manifest
        .entry(HarnessKind::Fx)
        .expect("fx compatibility should exist");
    assert_eq!(fx.last_verified_version.to_string(), "0.0.3");

    let mut advanced = manifest;
    let claude = advanced
        .harnesses
        .iter_mut()
        .find(|entry| entry.id == HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility should be mutable");
    claude.last_verified_version = semver::Version::new(2, 1, 240);
    assert_eq!(
        advanced.classify(HarnessKind::ClaudeCode, &semver::Version::new(2, 1, 233)),
        Some(VersionStatus::Supported)
    );
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
    assert_eq!(report.minimum_supported_version.to_string(), "2.1.233");
    assert_eq!(report.last_verified_version.to_string(), "2.1.233");
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
    assert!(report.warnings[0].contains("last version verified"));
    assert!(report.warnings[0].contains("2.1.233"));
    assert!(report.warnings[0].contains("forward-compatible safeguards"));

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
