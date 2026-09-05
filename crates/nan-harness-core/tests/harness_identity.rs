use std::str::FromStr;

use nan_harness_core::harness::{
    CompatibilityManifest, CompatibilityPolicy, CompatibilityStatus, CompatibilityTransport,
    HarnessCompatibility, NewerVersionPolicy, OlderVersionPolicy, UnparseableVersionPolicy,
};
use nan_harness_core::{HarnessKind, VersionStatus};
use semver::Version;

const KIND_CONTRACTS: [(HarnessKind, &str, &str); 15] = [
    (HarnessKind::ClaudeCode, "claude-code", "claude"),
    (HarnessKind::Codex, "codex", "codex"),
    (HarnessKind::OpenCode, "opencode", "opencode"),
    (HarnessKind::Hermes, "hermes", "hermes"),
    (HarnessKind::Pi, "pi", "pi"),
    (HarnessKind::Omp, "omp", "omp"),
    (HarnessKind::PrimeAgent, "prime-agent", "prime-agent"),
    (HarnessKind::DeepSeekHarness, "deepseek-harness", "dsh"),
    (HarnessKind::OpenClaw, "openclaw", "openclaw"),
    (HarnessKind::Cline, "cline", "cline"),
    (HarnessKind::QwenCode, "qwen-code", "qwen"),
    (HarnessKind::KimiCode, "kimi-code", "kimi"),
    (HarnessKind::Aider, "aider", "aider"),
    (HarnessKind::Goose, "goose", "goose"),
    (HarnessKind::Fx, "fx", "fx"),
];

const ALIAS_CONTRACTS: [(HarnessKind, &[&str]); 15] = [
    (HarnessKind::ClaudeCode, &["claude-code", "claude"]),
    (HarnessKind::Codex, &["codex"]),
    (HarnessKind::OpenCode, &["opencode"]),
    (HarnessKind::Hermes, &["hermes"]),
    (HarnessKind::Pi, &["pi"]),
    (HarnessKind::Omp, &["omp", "oh-my-pi"]),
    (HarnessKind::PrimeAgent, &["prime-agent", "prime"]),
    (
        HarnessKind::DeepSeekHarness,
        &["deepseek-harness", "deepseek", "dsh"],
    ),
    (HarnessKind::OpenClaw, &["openclaw", "claw"]),
    (HarnessKind::Cline, &["cline"]),
    (HarnessKind::QwenCode, &["qwen-code", "qwen"]),
    (HarnessKind::KimiCode, &["kimi-code", "kimi"]),
    (HarnessKind::Aider, &["aider"]),
    (HarnessKind::Goose, &["goose"]),
    (HarnessKind::Fx, &["fx"]),
];

const JSON_CONTRACTS: [(HarnessKind, &str); 15] = [
    (HarnessKind::ClaudeCode, "\"claude-code\""),
    (HarnessKind::Codex, "\"codex\""),
    (HarnessKind::OpenCode, "\"opencode\""),
    (HarnessKind::Hermes, "\"hermes\""),
    (HarnessKind::Pi, "\"pi\""),
    (HarnessKind::Omp, "\"omp\""),
    (HarnessKind::PrimeAgent, "\"prime-agent\""),
    (HarnessKind::DeepSeekHarness, "\"deepseek-harness\""),
    (HarnessKind::OpenClaw, "\"openclaw\""),
    (HarnessKind::Cline, "\"cline\""),
    (HarnessKind::QwenCode, "\"qwen-code\""),
    (HarnessKind::KimiCode, "\"kimi-code\""),
    (HarnessKind::Aider, "\"aider\""),
    (HarnessKind::Goose, "\"goose\""),
    (HarnessKind::Fx, "\"fx\""),
];

const VERSION_STATUS_CONTRACTS: [(VersionStatus, &str); 5] = [
    (VersionStatus::Tested, "\"tested\""),
    (VersionStatus::Supported, "\"supported\""),
    (VersionStatus::NewerUntested, "\"newer-untested\""),
    (VersionStatus::OlderUnsupported, "\"older-unsupported\""),
    (VersionStatus::Unparseable, "\"unparseable\""),
];

const REJECTED_HARNESS_NAMES: [&str; 8] = [
    "",
    "claudecode",
    "Claude-Code",
    "Codex",
    "oh my pi",
    "prime-agent ",
    "dsh2",
    "zed",
];

#[test]
fn canonical_display_names_and_binary_names_are_stable() {
    for (kind, display_name, binary_name) in KIND_CONTRACTS {
        assert_eq!(kind.to_string(), display_name);
        assert_eq!(kind.binary_name(), binary_name);
    }
}

#[test]
fn canonical_names_and_aliases_parse_to_their_harness() {
    for (kind, aliases) in ALIAS_CONTRACTS {
        for alias in aliases {
            let parsed = HarnessKind::from_str(alias);
            assert_eq!(
                parsed,
                Ok(kind),
                "alias `{alias}` should parse to its harness"
            );
        }
    }
}

#[test]
fn unsupported_names_report_the_requested_name() {
    for rejected in REJECTED_HARNESS_NAMES {
        let error =
            HarnessKind::from_str(rejected).expect_err("a rejected harness name should not parse");

        assert!(error.to_string().contains(rejected));
    }
}

#[test]
fn all_contains_each_harness_kind_once() {
    let expected = KIND_CONTRACTS.map(|(kind, _, _)| kind);

    assert_eq!(HarnessKind::ALL, expected);
}

#[test]
fn harness_kinds_round_trip_through_canonical_json_names() {
    for (kind, expected_json) in JSON_CONTRACTS {
        let actual_json =
            serde_json::to_string(&kind).expect("a harness kind should serialize as JSON");

        assert_eq!(actual_json, expected_json);
        assert_eq!(
            serde_json::from_str::<HarnessKind>(&actual_json)
                .expect("a canonical harness JSON name should deserialize"),
            kind
        );
    }

    assert!(serde_json::from_str::<HarnessKind>("\"claude\"").is_err());
}

#[test]
fn version_statuses_round_trip_through_typed_json_names() {
    for (status, expected_json) in VERSION_STATUS_CONTRACTS {
        let actual_json =
            serde_json::to_string(&status).expect("a version status should serialize as JSON");

        assert_eq!(actual_json, expected_json);
        assert_eq!(
            serde_json::from_str::<VersionStatus>(&actual_json)
                .expect("a version status JSON name should deserialize"),
            status
        );
    }
}

#[test]
fn manifest_version_bounds_are_inclusive() {
    let manifest = manifest_with_bounds(
        HarnessKind::Codex,
        Version::new(1, 2, 0),
        Version::new(2, 0, 0),
    );

    assert_eq!(
        manifest.classify(HarnessKind::Codex, &Version::new(1, 1, 999)),
        Some(VersionStatus::OlderUnsupported)
    );
    assert_eq!(
        manifest.classify(HarnessKind::Codex, &Version::new(1, 2, 0)),
        Some(VersionStatus::Supported)
    );
    assert_eq!(
        manifest.classify(HarnessKind::Codex, &Version::new(1, 9, 999)),
        Some(VersionStatus::Supported)
    );
    assert_eq!(
        manifest.classify(HarnessKind::Codex, &Version::new(2, 0, 0)),
        Some(VersionStatus::Tested)
    );
    assert_eq!(
        manifest.classify(HarnessKind::Codex, &Version::new(2, 0, 1)),
        Some(VersionStatus::NewerUntested)
    );
}

#[test]
fn pinned_manifest_classifies_only_the_exact_version_as_tested() {
    let manifest = manifest_with_bounds(
        HarnessKind::DeepSeekHarness,
        Version::new(3, 4, 0),
        Version::new(3, 4, 0),
    );

    assert_eq!(
        manifest.classify(HarnessKind::DeepSeekHarness, &Version::new(3, 3, 999)),
        Some(VersionStatus::OlderUnsupported)
    );
    assert_eq!(
        manifest.classify(HarnessKind::DeepSeekHarness, &Version::new(3, 4, 0)),
        Some(VersionStatus::Tested)
    );
    assert_eq!(
        manifest.classify(HarnessKind::DeepSeekHarness, &Version::new(3, 4, 1)),
        Some(VersionStatus::NewerUntested)
    );
}

#[test]
fn manifest_classifies_an_unregistered_harness_as_none() {
    let manifest = manifest_with_bounds(
        HarnessKind::Codex,
        Version::new(1, 2, 0),
        Version::new(2, 0, 0),
    );

    assert_eq!(
        manifest.classify(HarnessKind::ClaudeCode, &Version::new(1, 2, 0)),
        None
    );
}

fn manifest_with_bounds(
    kind: HarnessKind,
    minimum_version: Version,
    last_compatible_version: Version,
) -> CompatibilityManifest {
    CompatibilityManifest {
        schema_version: 3,
        tested_at: "2026-01-01T00:00:00Z".to_owned(),
        policy: CompatibilityPolicy {
            newer: NewerVersionPolicy::AllowWithWarning,
            older: OlderVersionPolicy::RequireAllowUnsupported,
            unparseable: UnparseableVersionPolicy::ConfirmOrRequireAllowUntested,
        },
        harnesses: vec![HarnessCompatibility {
            id: kind,
            command: "test-harness".to_owned(),
            last_compatible_version,
            compatible_at: "2026-01-01T00:00:00Z".to_owned(),
            last_live_verified_version: None,
            live_verified_at: None,
            minimum_version,
            runtime: None,
            transport: CompatibilityTransport::DirectChat,
            status: CompatibilityStatus::Verified,
        }],
    }
}
