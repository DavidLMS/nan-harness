use std::path::Path;
use std::process::{Command, Output, Stdio};

pub fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nanh"));
    command
        .env_clear()
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("APPDATA", root.join("appdata"))
        .env("LOCALAPPDATA", root.join("localappdata"))
        .env("XDG_CONFIG_HOME", root.join("xdg"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("PATH", root.join("bin"))
        .env("NAN_HARNESS_CONFIG_DIR", root.join("state"))
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .stdin(Stdio::null());
    // Native Windows processes need the OS directory, never the user's environment.
    #[cfg(windows)]
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

pub fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for directory in [
        "home",
        "appdata",
        "localappdata",
        "xdg",
        "bin",
        "state",
        "tmp",
    ] {
        std::fs::create_dir(root.path().join(directory)).unwrap();
    }
    root
}

pub fn diagnostics(root: &Path, arguments: &[&str]) -> Output {
    command(root)
        .arg("diagnostics")
        .args(arguments)
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .output()
        .expect("diagnostics should run")
}

pub fn assert_success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

pub fn native_harness(root: &Path) {
    let executable = root
        .join("bin")
        .join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    // Compile a tiny native executable so Windows exercises real executable discovery.
    let output = Command::new("rustc")
        .arg("--edition=2024")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/diagnostic_contracts/native_harness.rs"),
        )
        .arg("-o")
        .arg(executable)
        .output()
        .expect("pinned Rust compiler should be available");
    assert_success(&output);
}
