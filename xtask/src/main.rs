mod changelog;
mod dependencies;
mod release;

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [task] if task == "check" => check(),
        [task] if task == "changelog-check" => release::validate_changelog(),
        [task] if task == "dependency-check" => dependencies::check(),
        [task, version] if task == "set-version" => release::set_version(version),
        [task, version, output] if task == "changelog-notes" => {
            release::write_changelog_notes(version, Path::new(output))
        }
        [task, tag] if task == "release-check" => release::validate_tag(tag),
        [task, tag, repository, directory] if task == "release-metadata" => {
            release::generate_metadata(tag, repository, Path::new(directory))
        }
        [task, output] if task == "compatibility-feed" => {
            release::generate_compatibility_feed(Path::new(output))
        }
        [task, base, updates, output] if task == "merge-compatibility-feed" => {
            release::merge_compatibility_feed(
                Path::new(base),
                Path::new(updates),
                Path::new(output),
            )
        }
        [task, input] if task == "validate-compatibility-feed" => {
            release::validate_compatibility_feed(Path::new(input))
        }
        [task] if task == "help" => {
            print_help();
            Ok(())
        }
        [] => {
            print_help();
            Ok(())
        }
        [unknown, ..] => Err(format!("invalid arguments for task '{unknown}'")),
    }
}

fn check() -> Result<(), String> {
    release::validate_changelog()?;
    run_canary_contracts()?;
    run_cargo(["fmt", "--all", "--", "--check"], None)?;
    run_cargo(
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        None,
    )?;
    run_cargo(["test", "--workspace", "--all-features"], None)?;
    run_cargo(
        ["doc", "--workspace", "--no-deps"],
        Some(("RUSTDOCFLAGS", "-Dwarnings")),
    )?;
    run_cargo(["deny", "check"], None).and_then(|()| dependencies::check())
}

#[cfg(unix)]
fn run_canary_contracts() -> Result<(), String> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask manifest has no repository parent".to_owned())?;
    let status = Command::new("bash")
        .arg("canary/tests/all.sh")
        .current_dir(repository)
        .status()
        .map_err(|error| format!("could not start canary shell contracts: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("canary shell contracts exited with {status}"))
    }
}

#[cfg(not(unix))]
fn run_canary_contracts() -> Result<(), String> {
    println!("Skipping Unix canary shell contracts on this platform");
    Ok(())
}

fn run_cargo<const N: usize>(
    arguments: [&str; N],
    environment: Option<(&str, &str)>,
) -> Result<(), String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(&cargo);
    command.args(arguments);

    if let Some((key, value)) = environment {
        command.env(key, value);
    }

    let status = command.status().map_err(|error| {
        format!(
            "could not start {}: {error}",
            display_command(&cargo, &arguments)
        )
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} exited with {status}",
            display_command(&cargo, &arguments)
        ))
    }
}

fn display_command(cargo: &OsStr, arguments: &[&str]) -> String {
    let cargo = cargo.to_string_lossy();
    format!("{cargo} {}", arguments.join(" "))
}

fn print_help() {
    println!("Repository tasks for nan-harness");
    println!();
    println!("Usage: cargo xtask <TASK>");
    println!();
    println!("Tasks:");
    println!("  check                                      Run all repository quality gates");
    println!("  changelog-check                            Validate current release notes");
    println!("  changelog-notes <VERSION> <FILE>           Extract notes for one release");
    println!("  dependency-check                           Validate reviewed dependency paths");
    println!("  set-version <VERSION_OR_TAG>              Prepare version and changelog metadata");
    println!("  release-check <TAG>                        Validate a release tag");
    println!("  release-metadata <TAG> <REPOSITORY> <DIR>  Build verified release metadata");
    println!(
        "  compatibility-feed <FILE>                  Build the release-scoped compatibility feed"
    );
    println!(
        "  merge-compatibility-feed <BASE> <DIR> <FILE> Merge successful compatibility evidence"
    );
    println!(
        "  validate-compatibility-feed <FILE>          Validate a schema-v2 compatibility feed"
    );
    println!("  help                                       Print this help");
}
