use super::{HarnessIdentitySource, normalized_version, telemetry_harness_identity};
use crate::app::{AuthCommand, Cli, Command};
use clap::Parser as _;
use nan_harness_core::{DetectedHarness, HarnessKind, VersionStatus};
use nan_harness_telemetry::event::{
    CompatibilityStatus as TelemetryCompatibilityStatus, HarnessKind as TelemetryHarnessKind,
};
use std::collections::BTreeSet;

const STABLE_TARGETS: [(&str, TelemetryHarnessKind); 15] = [
    ("claude", TelemetryHarnessKind::ClaudeCode),
    ("codex", TelemetryHarnessKind::Codex),
    ("opencode", TelemetryHarnessKind::OpenCode),
    ("hermes", TelemetryHarnessKind::Hermes),
    ("pi", TelemetryHarnessKind::Pi),
    ("omp", TelemetryHarnessKind::Omp),
    ("prime", TelemetryHarnessKind::PrimeAgent),
    ("dsh", TelemetryHarnessKind::DeepSeekHarness),
    ("openclaw", TelemetryHarnessKind::OpenClaw),
    ("cline", TelemetryHarnessKind::Cline),
    ("qwen", TelemetryHarnessKind::QwenCode),
    ("kimi", TelemetryHarnessKind::KimiCode),
    ("aider", TelemetryHarnessKind::Aider),
    ("goose", TelemetryHarnessKind::Goose),
    ("fx", TelemetryHarnessKind::Fx),
];

const DESKTOP_TARGETS: [(&str, TelemetryHarnessKind); 5] = [
    ("chatgpt-desktop", TelemetryHarnessKind::ChatGptDesktop),
    ("claude-desktop", TelemetryHarnessKind::ClaudeDesktop),
    ("hermes-desktop", TelemetryHarnessKind::HermesDesktop),
    ("pen", TelemetryHarnessKind::PenDesktop),
    ("zed", TelemetryHarnessKind::ZedDesktop),
];

#[test]
fn harness_commands_map_to_their_typed_telemetry_identity() {
    for (command, expected) in STABLE_TARGETS.iter().chain(&DESKTOP_TARGETS) {
        let cli =
            Cli::try_parse_from(["nan-harness", command]).expect("harness command should parse");
        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly)
            .unwrap_or_else(|| panic!("{command} should identify a telemetry harness"));

        assert_eq!(identity.kind(), *expected, "{command} identity kind");
        assert_eq!(identity.version(), None, "{command} kind-only identity");
        assert_eq!(
            identity.compatibility(),
            None,
            "{command} kind-only identity must not infer compatibility"
        );
    }
}

#[test]
fn doctor_targets_map_to_their_typed_telemetry_identity() {
    for (target, expected) in STABLE_TARGETS.iter().chain(&DESKTOP_TARGETS) {
        let cli = Cli::try_parse_from(["nan-harness", "doctor", target])
            .expect("doctor command should parse");
        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly)
            .unwrap_or_else(|| panic!("doctor {target} should identify a telemetry harness"));

        assert_eq!(identity.kind(), *expected, "doctor {target} identity kind");
        assert_eq!(
            identity.version(),
            None,
            "doctor {target} kind-only identity"
        );
        assert_eq!(
            identity.compatibility(),
            None,
            "doctor {target} kind-only identity must not infer compatibility"
        );
    }
}

#[test]
fn doctor_without_a_target_has_no_harness_identity() {
    let cli = Cli::try_parse_from(["nan-harness", "doctor"]).expect("doctor command should parse");

    assert_eq!(
        telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly),
        None
    );
}

#[test]
fn config_targets_map_to_their_typed_telemetry_identity() {
    for (target, expected) in &STABLE_TARGETS {
        let cli = Cli::try_parse_from(["nan-harness", "config", target])
            .expect("config command should parse");
        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly)
            .unwrap_or_else(|| panic!("config {target} should identify a telemetry harness"));

        assert_eq!(identity.kind(), *expected, "config {target} identity kind");
    }

    let cli = Cli::try_parse_from(["nan-harness", "config", "pen"])
        .expect("Pen config command should parse");
    let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly)
        .expect("Pen config should identify a telemetry harness");

    assert_eq!(
        identity.kind(),
        TelemetryHarnessKind::PenDesktop,
        "Pen config identity kind"
    );
}

#[test]
fn config_without_a_target_has_no_harness_identity() {
    let cli = Cli::try_parse_from(["nan-harness", "config"]).expect("config command should parse");

    assert_eq!(
        telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly),
        None
    );
}

#[test]
fn administrative_commands_have_no_harness_identity() {
    let commands: [&[&str]; 7] = [
        &["nan-harness", "update"],
        &["nan-harness", "uninstall", "--yes"],
        &["nan-harness", "telemetry", "off"],
        &["nan-harness", "completions", "bash"],
        &["nan-harness", "diagnostics", "status"],
        &["nan-harness", "__coordinator"],
        &[
            "nan-harness",
            "__record-installation",
            "--executable",
            "synthetic-executable",
            "--alias",
            "synthetic-alias",
        ],
    ];
    let auth_cli = Cli {
        command: Command::Auth {
            command: AuthCommand::Status,
        },
    };

    assert_eq!(
        telemetry_harness_identity(&auth_cli, HarnessIdentitySource::KindOnly),
        None,
        "auth must not attach a harness identity"
    );

    for command in commands {
        let cli = Cli::try_parse_from(command)
            .unwrap_or_else(|error| panic!("{command:?} should parse: {error}"));
        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::KindOnly);

        assert!(
            identity.is_none(),
            "{command:?} must not attach a harness identity"
        );
    }
}

#[test]
fn known_harness_version_statuses_map_to_typed_telemetry_compatibility() {
    let cases = [
        (VersionStatus::Tested, TelemetryCompatibilityStatus::Tested),
        (
            VersionStatus::Supported,
            TelemetryCompatibilityStatus::Supported,
        ),
        (
            VersionStatus::NewerUntested,
            TelemetryCompatibilityStatus::NewerUntested,
        ),
        (
            VersionStatus::OlderUnsupported,
            TelemetryCompatibilityStatus::OlderUnsupported,
        ),
        (
            VersionStatus::Unparseable,
            TelemetryCompatibilityStatus::Unparseable,
        ),
    ];

    for (source_status, expected) in cases {
        let detected = DetectedHarness {
            kind: HarnessKind::ClaudeCode,
            executable: "synthetic-claude".to_owned(),
            detected_version: "claude 1.2.3".to_owned(),
            version_status: source_status,
            capabilities: BTreeSet::new(),
        };
        let cli =
            Cli::try_parse_from(["nan-harness", "claude"]).expect("Claude command should parse");
        let identity = telemetry_harness_identity(&cli, HarnessIdentitySource::Known(&detected))
            .expect("known harness identity should be retained");

        assert_eq!(identity.kind(), TelemetryHarnessKind::ClaudeCode);
        assert_eq!(identity.version(), Some("1.2.3"));
        assert_eq!(
            identity.compatibility(),
            Some(expected),
            "{source_status:?} compatibility mapping"
        );
    }
}

#[test]
fn normalized_version_selects_and_normalizes_a_semantic_version_token() {
    let cases = [
        ("claude v1.2.3", Some("1.2.3".to_owned())),
        ("[v1.2.3]", Some("1.2.3".to_owned())),
        (
            "hermes 1.2.3-beta.1+build.1",
            Some("1.2.3-beta.1+build.1".to_owned()),
        ),
        ("harness 1.2", None),
        ("", None),
    ];

    for (output, expected) in cases {
        assert_eq!(normalized_version(output), expected, "{output:?} version");
    }
}
