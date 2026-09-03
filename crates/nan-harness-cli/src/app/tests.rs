use super::{Cli, Command, DoctorTarget};
use clap::{CommandFactory as _, Parser as _, error::ErrorKind};
use nan_harness_core::DesktopHarnessKind;

#[test]
fn bare_invocation_displays_full_help_with_the_existing_error_code() {
    let error = Cli::try_parse_from(["nanh"]).expect_err("a subcommand is still required");

    assert_eq!(
        error.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("Usage: nan-harness <COMMAND>"));
}

#[test]
fn top_level_help_includes_quickstart_examples() {
    let help = Cli::command()
        .get_after_help()
        .expect("top-level help should include examples")
        .to_string();

    assert!(help.contains("Examples:"));
    assert!(help.contains("nanh claude"));
    assert!(help.contains("nanh doctor"));
}

#[test]
fn mistyped_harness_suggests_the_nearest_command() {
    let error = Cli::try_parse_from(["nanh", "cluade"]).expect_err("unknown command should fail");

    assert!(error.to_string().contains("claude"));
}

#[test]
fn config_accepts_the_same_search_policy_flags_as_launches() {
    let disabled = Cli::try_parse_checked_from(["nan-harness", "config", "cline", "--no-search"])
        .expect("disabled search policy should parse");
    let Command::Config(disabled) = disabled.command else {
        panic!("config command should parse");
    };
    assert!(disabled.search.no_search);
    assert!(!disabled.search.force_search);

    let forced = Cli::try_parse_checked_from(["nan-harness", "config", "cline", "--force-search"])
        .expect("forced search policy should parse");
    let Command::Config(forced) = forced.command else {
        panic!("config command should parse");
    };
    assert!(!forced.search.no_search);
    assert!(forced.search.force_search);

    assert!(
        Cli::try_parse_checked_from([
            "nan-harness",
            "config",
            "cline",
            "--no-search",
            "--force-search",
        ])
        .is_err()
    );
}

#[test]
fn desktop_commands_are_visible_typed_and_keep_restore_exclusive() {
    let help = Cli::command().render_long_help().to_string();
    for command in [
        "chatgpt-desktop",
        "codex-desktop",
        "claude-desktop",
        "hermes-desktop",
        "pen",
        "pen-desktop",
        "zed",
        "zed-desktop",
    ] {
        assert!(help.contains(command), "missing Desktop command {command}");
    }

    let doctor = Cli::try_parse_checked_from(["nan-harness", "doctor", "codex-desktop", "--json"])
        .expect("Desktop doctor alias should parse");
    let Command::Doctor(doctor) = doctor.command else {
        panic!("doctor command should parse");
    };
    assert_eq!(
        doctor.harness,
        Some(DoctorTarget::Experimental(DesktopHarnessKind::ChatGpt))
    );

    assert!(
        Cli::try_parse_checked_from([
            "nan-harness",
            "claude-desktop",
            "--restore",
            "--model",
            "qwen3.6",
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_checked_from(["nan-harness", "zed", "--restore", "--model", "qwen3.6",])
            .is_err()
    );
    assert!(
        Cli::try_parse_checked_from([
            "nan-harness",
            "hermes-desktop",
            "--restore",
            "--",
            "--source",
        ])
        .is_err()
    );
}

#[test]
fn hermes_desktop_configuration_is_an_exact_parser_alias() {
    for name in ["hermes", "hermes-desktop"] {
        let cli = Cli::try_parse_checked_from(["nan-harness", "config", name, "--status"])
            .expect("Hermes config spelling should parse");
        let Command::Config(arguments) = cli.command else {
            panic!("config command should parse");
        };
        assert_eq!(
            arguments.harness,
            Some(super::ConfigTarget::Stable(
                nan_harness_core::HarnessKind::Hermes
            ))
        );
        assert!(arguments.status);
    }
}

#[test]
fn pen_configuration_accepts_desktop_and_short_names() {
    for name in ["pen", "pen-desktop"] {
        let cli = Cli::try_parse_checked_from(["nan-harness", "config", name, "--status"])
            .expect("Pen config spelling should parse");
        let Command::Config(arguments) = cli.command else {
            panic!("config command should parse");
        };
        assert_eq!(arguments.harness, Some(super::ConfigTarget::Pen));
        assert!(arguments.status);
    }
}
