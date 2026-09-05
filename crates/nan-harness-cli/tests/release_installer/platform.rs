use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn assert_version(binary: &Path) {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .expect("installed binary should start");
    assert_success("installed binary", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("nan-harness {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
pub(crate) fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    assert_eq!(
        fs::read_link(&alias).expect("alias should be a symbolic link"),
        PathBuf::from("nan-harness")
    );
    assert_version(&alias);
}

#[cfg(windows)]
pub(crate) fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    let contents = fs::read_to_string(&alias).expect("command alias should be readable");
    assert_eq!(contents, "@echo off\r\n\"%~dp0nan-harness.exe\" %*\r\n");
    let output = Command::new("cmd.exe")
        .args(["/D", "/C"])
        .arg(&alias)
        .arg("--version")
        .output()
        .expect("command alias should start");
    assert_success("command alias", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("nan-harness {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
pub(crate) fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh")
}

#[cfg(windows)]
pub(crate) fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh.cmd")
}

#[cfg(unix)]
pub(crate) fn conflicting_nanh_path(install_directory: &Path) -> PathBuf {
    alias_path(install_directory)
}

#[cfg(windows)]
pub(crate) fn conflicting_nanh_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh.exe")
}

#[cfg(unix)]
pub(crate) fn unrelated_nan_paths(install_directory: &Path) -> Vec<PathBuf> {
    vec![install_directory.join("nan")]
}

#[cfg(windows)]
pub(crate) fn unrelated_nan_paths(install_directory: &Path) -> Vec<PathBuf> {
    vec![
        install_directory.join("nan.exe"),
        install_directory.join("nan.cmd"),
    ]
}

#[cfg(unix)]
pub(crate) fn previous_alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nan")
}

#[cfg(windows)]
pub(crate) fn previous_alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nan.cmd")
}

#[cfg(unix)]
pub(crate) fn write_previous_managed_alias(install_directory: &Path) {
    std::os::unix::fs::symlink("nan-harness", previous_alias_path(install_directory))
        .expect("previous managed alias should be writable");
}

#[cfg(windows)]
pub(crate) fn write_previous_managed_alias(install_directory: &Path) {
    fs::write(
        previous_alias_path(install_directory),
        b"@echo off\r\n\"%~dp0nan-harness.exe\" %*\r\n",
    )
    .expect("previous managed alias should be writable");
}

#[cfg(windows)]
pub(crate) const fn installer_file_name() -> &'static str {
    "install.ps1"
}

#[cfg(not(windows))]
pub(crate) const fn installer_file_name() -> &'static str {
    "install.sh"
}

#[cfg(windows)]
pub(crate) const fn binary_file_name() -> &'static str {
    "nan-harness.exe"
}

#[cfg(not(windows))]
pub(crate) const fn binary_file_name() -> &'static str {
    "nan-harness"
}

#[cfg(unix)]
pub(crate) fn installer_command(script: &Path) -> Command {
    let mut command = Command::new("sh");
    command.arg(script);
    command
}

#[cfg(windows)]
pub(crate) fn installer_command(script: &Path) -> Command {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ]);
    command.arg(script);
    command
}
