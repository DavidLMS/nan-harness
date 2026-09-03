use crate::app::{Cli, CompletionShell};
use clap::CommandFactory as _;
use std::io::Write;

pub(crate) fn run(shell: CompletionShell) {
    write(shell, &mut std::io::stdout());
}

fn write(shell: CompletionShell, output: &mut dyn Write) {
    let mut command = completion_command();
    clap_complete::generate(
        clap_complete::Shell::from(shell),
        &mut command,
        "nanh",
        output,
    );
}

fn completion_command() -> clap::Command {
    let command = Cli::command();
    let public_subcommands = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .cloned();
    clap::Command::new("nanh")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Run AI coding harnesses through the NaN provider")
        .subcommand_required(true)
        .subcommands(public_subcommands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_shell_contains_public_commands_and_flags_only() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
            CompletionShell::Powershell,
        ] {
            let mut output = Vec::new();
            write(shell, &mut output);
            let script = String::from_utf8(output).expect("completion script should be UTF-8");

            assert!(!script.is_empty());
            for expected in ["claude", "codex", "prime", "deepseek", "qwen-code"] {
                assert!(
                    script.contains(expected),
                    "{shell:?} completion should contain {expected}"
                );
            }
            let expected_flags = if matches!(shell, CompletionShell::Fish) {
                ["-l model", "-l dry-run", "-l executable"]
            } else {
                ["--model", "--dry-run", "--executable"]
            };
            for expected in expected_flags {
                assert!(
                    script.contains(expected),
                    "{shell:?} completion should contain {expected}"
                );
            }
            assert!(
                !script.contains("__record-installation"),
                "{shell:?} completion must exclude hidden internals"
            );
        }
    }
}
