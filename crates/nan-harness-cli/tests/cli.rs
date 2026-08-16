use std::process::{Command, Output};

fn run(argument: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .arg(argument)
        .output()
        .expect("nan-harness should start")
}

#[test]
fn help_is_english() {
    let output = run("--help");
    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Run AI coding harnesses through NaN"));
    assert!(stdout.contains("Usage: nan-harness"));
    assert!(stdout.contains("Options:"));
}

#[test]
fn version_matches_the_workspace() {
    let output = run("--version");
    let stdout = String::from_utf8(output.stdout).expect("version output should be UTF-8");

    assert!(output.status.success());
    assert_eq!(stdout.trim(), "nan-harness 0.1.0");
}
