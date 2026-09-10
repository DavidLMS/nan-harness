use super::super::{
    ZedDesktopError, append_new_diagnostics, completed_session, exit_outcome, resolve_workspace,
    validate_compatibility,
};
use nan_harness_core::{DesktopHarnessKind, DesktopTransport};
use nan_harness_runtime::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint, DesktopCompatibilityEntry,
    DesktopCompatibilityEvidence, DesktopEvidenceSource, ExecutionOutcome, ProviderUsageSnapshot,
};
use semver::Version;

fn compatibility_entry(
    evidence: DesktopCompatibilityEvidence,
    minimum: Option<Version>,
    last_compatible: Option<Version>,
) -> DesktopCompatibilityEntry {
    DesktopCompatibilityEntry {
        id: DesktopHarnessKind::Zed,
        platform: "macos".to_owned(),
        transport: DesktopTransport::ChatCompletionsGateway,
        evidence,
        minimum_app_version: minimum,
        last_compatible_app_version: last_compatible,
        minimum_runtime_version: None,
        last_compatible_runtime_version: None,
        compatible_at: "2026-09-03".to_owned(),
        source: DesktopEvidenceSource::EmbeddedRegistry,
    }
}

fn live_verified_entry() -> DesktopCompatibilityEntry {
    compatibility_entry(
        DesktopCompatibilityEvidence::LiveVerified,
        Some(Version::new(1, 18, 0)),
        Some(Version::new(1, 18, 0)),
    )
}

fn diagnostic(code: &'static str) -> BridgeDiagnostic {
    BridgeDiagnostic {
        code,
        reason: BridgeDiagnosticReason::UpstreamStatus,
        http_status: Some(503),
        endpoint: BridgeEndpoint::Models,
        model_id: None,
        requested_reasoning: None,
        model_policy: None,
        timeout_phase: None,
        recovery_outcome: None,
        attempt: None,
        priority: None,
        cache_replay_detected: None,
        cache_bypass_attempted: None,
    }
}

#[test]
fn the_compatibility_gate_allows_newer_versions_and_enforces_the_minimum() {
    let entry = live_verified_entry();
    let older = Version::new(1, 17, 0);
    let newer = Version::new(1, 19, 0);

    validate_compatibility(&entry, Some(&Version::new(1, 18, 0)), false, false)
        .expect("the live-verified version should launch");
    validate_compatibility(&entry, None, false, false)
        .expect("an unknown version should not be classified as unsupported");
    assert!(matches!(
        validate_compatibility(&entry, Some(&older), false, false),
        Err(ZedDesktopError::OlderUnsupported)
    ));
    validate_compatibility(&entry, Some(&older), true, false)
        .expect("--allow-unsupported should override the minimum version");
    validate_compatibility(&entry, Some(&newer), false, false)
        .expect("newer versions should launch without an override");
    validate_compatibility(&entry, Some(&newer), false, true)
        .expect("the legacy flag should remain accepted");
}

#[test]
fn contract_only_platforms_launch_and_unavailable_platforms_do_not() {
    let contract_only = compatibility_entry(DesktopCompatibilityEvidence::ContractOnly, None, None);
    validate_compatibility(&contract_only, Some(&Version::new(2, 0, 0)), false, false)
        .expect("a contract-only platform should still launch");

    let unavailable = compatibility_entry(
        DesktopCompatibilityEvidence::Unavailable,
        Some(Version::new(1, 18, 0)),
        Some(Version::new(1, 18, 0)),
    );
    assert!(matches!(
        validate_compatibility(&unavailable, None, true, true),
        Err(ZedDesktopError::UnsupportedPlatform)
    ));
}

#[test]
fn workspace_resolution_prefers_absolute_paths_over_the_current_directory() {
    let current = std::env::current_dir().expect("the test process should have a directory");
    let directory = tempfile::tempdir().expect("temporary directory should be created");

    assert_eq!(
        resolve_workspace(Some(directory.path())).expect("an absolute directory should resolve"),
        directory.path()
    );
    assert_eq!(
        resolve_workspace(None).expect("the current directory should resolve"),
        current
    );
    assert_eq!(
        resolve_workspace(Some(std::path::Path::new("src")))
            .expect("a relative directory should resolve"),
        current.join("src")
    );
    assert!(matches!(
        resolve_workspace(Some(&directory.path().join("missing"))),
        Err(ZedDesktopError::InvalidWorkspace)
    ));
}

#[test]
fn a_session_error_takes_precedence_over_a_shutdown_failure() {
    let error = completed_session(
        Err(ZedDesktopError::DidNotStart),
        Err(ZedDesktopError::DidNotTerminate),
        None,
    )
    .expect_err("the session error should win");
    assert!(matches!(error, ZedDesktopError::DidNotStart));

    let error = completed_session(Ok(0), Err(ZedDesktopError::DidNotTerminate), None)
        .expect_err("a shutdown failure should surface when the session succeeded");
    assert!(matches!(error, ZedDesktopError::DidNotTerminate));

    let completed = completed_session(
        Ok(3),
        Ok((
            vec![diagnostic("NH-TEST-001")],
            ProviderUsageSnapshot::default(),
        )),
        None,
    )
    .expect("a clean shutdown should report the exit code");
    assert_eq!(completed.code, 3);
    assert_eq!(completed.diagnostics, vec![diagnostic("NH-TEST-001")]);
}

#[test]
fn diagnostics_are_appended_once_and_exit_codes_map_to_outcomes() {
    let mut collected = vec![diagnostic("NH-TEST-001")];
    append_new_diagnostics(
        &mut collected,
        vec![
            diagnostic("NH-TEST-001"),
            diagnostic("NH-TEST-002"),
            diagnostic("NH-TEST-002"),
        ],
    );
    assert_eq!(
        collected,
        vec![diagnostic("NH-TEST-001"), diagnostic("NH-TEST-002")]
    );

    assert_eq!(exit_outcome(0), ExecutionOutcome::Succeeded);
    assert_eq!(exit_outcome(1), ExecutionOutcome::Failed);
    assert_eq!(exit_outcome(-1), ExecutionOutcome::Failed);
}
