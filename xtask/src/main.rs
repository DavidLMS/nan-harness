use std::env;
use std::ffi::{OsStr, OsString};
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
    let mut arguments = env::args().skip(1);
    let task = arguments.next();

    if arguments.next().is_some() {
        return Err("xtask accepts exactly one task".to_owned());
    }

    match task.as_deref() {
        Some("check") => check(),
        Some("help") | None => {
            print_help();
            Ok(())
        }
        Some(unknown) => Err(format!("unknown task '{unknown}'")),
    }
}

fn check() -> Result<(), String> {
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
    run_cargo(["deny", "check"], None)
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
    println!("Repository tasks for NaN Harness");
    println!();
    println!("Usage: cargo xtask <TASK>");
    println!();
    println!("Tasks:");
    println!("  check    Run formatting, lint, test, documentation, and dependency gates");
    println!("  help     Print this help");
}
