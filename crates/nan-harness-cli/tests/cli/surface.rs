use crate::support::{run, run_alias};

#[test]
fn help_is_english_and_lists_engineering_commands() {
    let output = run(&["--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Run AI coding harnesses through the NaN provider"));
    assert!(stdout.contains("Usage: nan-harness <COMMAND>"));
    assert!(!stdout.contains("  run"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("uninstall"));
    assert!(stdout.contains("telemetry"));
    assert!(!stdout.contains("__record-installation"));
}

#[test]
fn harness_launch_commands_do_not_expose_configuration_mutation_flags() {
    for harness in [
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "dsh",
        "openclaw",
        "cline",
        "qwen",
        "kimi",
        "aider",
        "goose",
    ] {
        let output = run(&[harness, "--help"]);
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

        assert!(output.status.success());
        assert!(!stdout.contains("--persist"));
        assert!(!stdout.contains("--unpersist"));
    }
}

#[test]
fn every_harness_launch_exposes_mutually_exclusive_search_policy_flags() {
    for harness in [
        "claude",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "dsh",
        "openclaw",
        "cline",
        "qwen",
        "kimi",
        "aider",
        "goose",
        "fx",
    ] {
        let help = run(&[harness, "--help"]);
        let stdout = String::from_utf8(help.stdout).expect("help should be UTF-8");
        let normalized = stdout.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(help.status.success(), "{harness}");
        assert!(stdout.contains("--no-search"), "{harness}: {stdout}");
        assert!(stdout.contains("--force-search"), "{harness}: {stdout}");
        assert!(
            normalized
                .contains("Do not add NaN web search; preserve any existing search configuration"),
            "{harness}: {stdout}"
        );
        assert!(
            normalized
                .contains("Use NaN web search even when another search provider is configured"),
            "{harness}: {stdout}"
        );

        let conflict = run(&[harness, "--no-search", "--force-search"]);
        assert!(!conflict.status.success(), "{harness}");
    }

    let config_help = run(&["config", "--help"]);
    let stdout = String::from_utf8(config_help.stdout).expect("help should be UTF-8");
    assert!(config_help.status.success());
    assert!(stdout.contains("--no-search"));
    assert!(stdout.contains("--force-search"));
}

#[test]
fn gateway_escape_hatch_is_documented_only_for_direct_chat_commands() {
    for harness in [
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "dsh",
        "openclaw",
        "cline",
        "qwen",
        "kimi",
        "aider",
        "goose",
    ] {
        let output = run(&[harness, "--help"]);
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

        assert!(output.status.success());
        assert!(stdout.contains("--no-chat-gateway"), "{harness}: {stdout}");
        assert!(stdout.contains("Bypass the local Chat Completions gateway"));
    }

    for harness in ["claude", "codex", "fx"] {
        let output = run(&[harness, "--help"]);
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");

        assert!(output.status.success());
        assert!(!stdout.contains("--no-chat-gateway"), "{harness}: {stdout}");
    }
}

#[test]
fn root_help_lists_executable_commands_and_aliases() {
    let output = run(&["--help"]);
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    for harness in [
        "claude",
        "claude-code",
        "codex",
        "opencode",
        "hermes",
        "pi",
        "prime-agent",
        "prime",
        "dsh",
        "deepseek",
        "deepseek-harness",
        "openclaw",
        "cline",
        "qwen",
        "qwen-code",
        "kimi",
        "kimi-code",
        "aider",
        "goose",
    ] {
        assert!(stdout.contains(harness), "missing {harness} from root help");
    }
}

#[test]
fn nan_alias_exposes_the_same_command_surface() {
    let primary = run(&["--help"]);
    let alias = run_alias(&["--help"]);
    let alias_help = String::from_utf8(alias.stdout).expect("alias help should be UTF-8");

    assert!(primary.status.success());
    assert!(alias.status.success());
    assert!(alias_help.contains("Usage: nan-harness <COMMAND>"));
    for command in ["claude", "codex", "goose", "doctor", "auth", "telemetry"] {
        assert!(
            alias_help.contains(command),
            "alias help is missing {command}"
        );
    }
    assert!(!alias_help.contains("  run"));
}

#[test]
fn version_matches_the_workspace() {
    let output = run(&["--version"]);
    let stdout = String::from_utf8(output.stdout).expect("version output should be UTF-8");

    assert!(output.status.success());
    assert_eq!(
        stdout.trim(),
        format!("nan-harness {}", env!("CARGO_PKG_VERSION"))
    );
}
