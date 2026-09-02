#![cfg(unix)]

use nan_harness_core::{HarnessCapability, HarnessKind, VersionStatus};
use nan_harness_runtime::{
    DiscoveryError, DiscoveryOptions, bundled_compatibility_manifest, discover_harness,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
fn bundled_manifest_is_typed_and_complete() {
    let manifest = bundled_compatibility_manifest().expect("manifest should parse");

    assert_eq!(manifest.schema_version, 3);
    assert_eq!(manifest.harnesses.len(), 15);
    let claude = manifest
        .entry(HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility should exist");
    assert_eq!(claude.minimum_version.to_string(), "2.1.233");
    assert_eq!(claude.last_compatible_version.to_string(), "2.1.251");
    assert_eq!(claude.compatible_at, "2026-08-29T00:00:00Z");
    assert_eq!(
        claude
            .last_live_verified_version
            .as_ref()
            .expect("live evidence")
            .to_string(),
        "2.1.233"
    );
    assert_eq!(
        claude.live_verified_at.as_deref(),
        Some("2026-08-18T00:00:00Z")
    );
    assert!(manifest.entry(HarnessKind::PrimeAgent).is_some());
    assert!(manifest.entry(HarnessKind::DeepSeekHarness).is_some());
    assert!(manifest.entry(HarnessKind::OpenClaw).is_some());
    assert!(manifest.entry(HarnessKind::Cline).is_some());
    assert!(manifest.entry(HarnessKind::QwenCode).is_some());
    assert!(manifest.entry(HarnessKind::KimiCode).is_some());
    assert!(manifest.entry(HarnessKind::Aider).is_some());
    assert!(manifest.entry(HarnessKind::Goose).is_some());
    assert!(manifest.entry(HarnessKind::Omp).is_some());
    let fx = manifest
        .entry(HarnessKind::Fx)
        .expect("fx compatibility should exist");
    assert_eq!(fx.last_compatible_version.to_string(), "0.0.7");
    assert_eq!(
        manifest
            .entry(HarnessKind::QwenCode)
            .expect("Qwen Code compatibility should exist")
            .last_compatible_version
            .to_string(),
        "0.22.0"
    );

    let mut advanced = manifest;
    let claude = advanced
        .harnesses
        .iter_mut()
        .find(|entry| entry.id == HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility should be mutable");
    claude.last_compatible_version = semver::Version::new(2, 1, 240);
    assert_eq!(
        advanced.classify(HarnessKind::ClaudeCode, &semver::Version::new(2, 1, 233)),
        Some(VersionStatus::Supported)
    );
}

#[test]
fn discovery_classifies_tested_newer_and_overridden_versions() {
    let baseline = fake_executable("claude 2.1.233");
    let baseline_report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&baseline),
        DiscoveryOptions::default(),
    )
    .expect("the minimum supported version should pass");
    let compatible_version = baseline_report.last_compatible_version;

    let tested = fake_executable(&format!("claude {compatible_version}"));
    let report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&tested),
        DiscoveryOptions::default(),
    )
    .expect("tested version should pass");
    assert_eq!(report.harness.version_status, VersionStatus::Tested);
    assert_eq!(report.minimum_supported_version.to_string(), "2.1.233");
    assert_eq!(report.last_compatible_version, compatible_version);
    assert!(report.warnings.is_empty());

    let mut newer_version = compatible_version.clone();
    newer_version.patch += 1;
    let newer = fake_executable(&format!("claude {newer_version}"));
    let report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&newer),
        DiscoveryOptions::default(),
    )
    .expect("newer version should pass with warning");
    assert_eq!(report.harness.version_status, VersionStatus::NewerUntested);
    assert_eq!(report.warnings.len(), 1);
    assert!(
        report.warnings[0]
            .contains("last version confirmed compatible with this nan-harness release")
    );
    assert!(report.warnings[0].contains(&compatible_version.to_string()));
    assert!(report.warnings[0].contains("forward-compatible safeguards"));
    assert!(
        report.warnings[0].contains(&format!("detected claude-code ({newer_version})")),
        "{}",
        report.warnings[0]
    );

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
fn discovery_honors_each_harness_version_command() {
    let executable = argument_sensitive_executable("--version", "dsh 0.1.0-rc.7");
    let report = discover_harness(
        HarnessKind::DeepSeekHarness,
        Some(&executable),
        DiscoveryOptions::default(),
    )
    .expect("the declared DeepSeek Harness version command should pass");

    // A valid local compatibility overlay may already have advanced beyond the
    // embedded fixture version, in which case this exact version is supported
    // rather than the newest tested one. Either status proves that discovery
    // used the harness-specific `--version` command successfully.
    assert!(matches!(
        report.harness.version_status,
        VersionStatus::Tested | VersionStatus::Supported
    ));
    assert_eq!(report.harness.detected_version, "dsh 0.1.0-rc.7");
}

#[test]
fn discovery_accepts_omp_slash_version_output() {
    let executable = argument_sensitive_executable("--version", "omp/18.0.11");
    let report = discover_harness(
        HarnessKind::Omp,
        Some(&executable),
        DiscoveryOptions::default(),
    )
    .expect("OMP's slash-delimited version output should pass");

    assert_eq!(report.harness.version_status, VersionStatus::Tested);
    assert_eq!(report.harness.detected_version, "omp/18.0.11");
}

#[test]
fn codex_capabilities_are_detected_from_the_installed_cli() {
    let supported = codex_executable(true);
    let report = discover_harness(
        HarnessKind::Codex,
        Some(&supported),
        DiscoveryOptions::default(),
    )
    .expect("Codex with profile support should be discovered");
    assert!(
        report
            .harness
            .capabilities
            .contains(&HarnessCapability::CodexConfigProfile)
    );
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| warning.contains("compatibility mode"))
    );

    let legacy = codex_executable(false);
    let report = discover_harness(
        HarnessKind::Codex,
        Some(&legacy),
        DiscoveryOptions::default(),
    )
    .expect("Codex without profile support should remain launchable");
    assert!(report.harness.capabilities.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("compatibility mode"))
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

#[test]
fn explicit_override_must_be_executable() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let executable = directory.path().join("fake-harness");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("fake executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o600))
        .expect("fake executable should not be executable");

    let result = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&executable),
        DiscoveryOptions::default(),
    );

    assert!(matches!(result, Err(DiscoveryError::InvalidExecutable(path)) if path == executable));
}

#[cfg(target_os = "linux")]
#[test]
fn discovery_retries_an_executable_that_is_temporarily_busy() {
    use std::fs::OpenOptions;
    use std::time::Duration;

    let tested_version = bundled_compatibility_manifest()
        .expect("bundled compatibility manifest should parse")
        .entry(HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility should exist")
        .last_compatible_version
        .clone();
    let executable = fake_executable(&format!("claude {tested_version}"));
    let writable_handle = OpenOptions::new()
        .write(true)
        .open(&executable)
        .expect("fixture should be opened for writing");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        drop(writable_handle);
    });

    let report = discover_harness(
        HarnessKind::ClaudeCode,
        Some(&executable),
        DiscoveryOptions::default(),
    )
    .expect("discovery should retry a transiently busy executable");
    release.join().expect("fixture handle should be released");

    assert_eq!(report.harness.version_status, VersionStatus::Tested);
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

fn argument_sensitive_executable(expected_argument: &str, version_output: &str) -> PathBuf {
    let directory = tempfile::tempdir()
        .expect("temporary directory should be created")
        .keep();
    let executable = directory.join("fake-harness");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\n[ \"${{1-}}\" = '{expected_argument}' ] || exit 23\nprintf '%s\\n' '{version_output}'\n"
        ),
    )
    .expect("fake executable should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}

fn codex_executable(supports_profiles: bool) -> PathBuf {
    let directory = tempfile::tempdir()
        .expect("temporary directory should be created")
        .keep();
    let executable = directory.join("codex");
    let profile_help = if supports_profiles {
        "printf '%s\\n' '  -p, --profile <CONFIG_PROFILE>'"
    } else {
        "printf '%s\\n' 'Codex help without profiles'"
    };
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\ncase \"${{1-}}\" in\n  --version) printf '%s\\n' 'codex-cli 0.146.0';;\n  --help) {profile_help};;\n  *) exit 23;;\nesac\n"
        ),
    )
    .expect("fake Codex should be written");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("fake Codex should be executable");
    executable
}
