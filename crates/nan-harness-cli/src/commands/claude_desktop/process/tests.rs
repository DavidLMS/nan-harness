#[cfg(unix)]
use super::super::error::ClaudeDesktopError;
#[cfg(unix)]
use super::{
    DesktopPlatform, DesktopProcess, SystemDesktopProcess, find_executable, is_executable_file,
    process_matches, run_launcher, terminate_matches,
};
use super::{find_versioned_windows_app, tasklist_reports_desktop};
use std::fs::{self, File};
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
struct Fixture {
    directory: TempDir,
    executable: PathBuf,
}

#[cfg(unix)]
impl Fixture {
    fn with_exit_code(code: i32) -> Self {
        let directory = tempfile::tempdir().expect("fixture directory");
        let executable = directory.path().join("fixture");
        fs::write(&executable, format!("#!/bin/sh\nexit {code}\n")).expect("fixture script");
        #[cfg(unix)]
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fixture permissions");
        Self {
            directory,
            executable,
        }
    }

    fn path(&self) -> &Path {
        &self.executable
    }

    fn missing_path(&self) -> PathBuf {
        self.directory.path().join("missing")
    }
}

#[cfg(unix)]
#[test]
fn process_matches_maps_exit_statuses_and_missing_commands() {
    assert!(matches!(
        process_matches(Fixture::with_exit_code(0).path().to_str().unwrap(), &[]),
        Ok(true)
    ));
    assert!(matches!(
        process_matches(Fixture::with_exit_code(1).path().to_str().unwrap(), &[]),
        Ok(false)
    ));

    let other = Fixture::with_exit_code(2);
    assert!(matches!(
        process_matches(other.path().to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::ProcessCheckFailed(Some(2)))
    ));

    let missing = Fixture::with_exit_code(0);
    assert!(matches!(
        process_matches(missing.missing_path().to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::ProcessCheck(_))
    ));
}

#[cfg(unix)]
#[test]
fn run_launcher_maps_success_nonzero_and_missing_commands() {
    let success = Fixture::with_exit_code(0);
    assert!(run_launcher(success.path().to_str().unwrap(), &[]).is_ok());

    let failure = Fixture::with_exit_code(7);
    assert!(matches!(
        run_launcher(failure.path().to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::LaunchFailed(Some(7)))
    ));

    let missing = Fixture::with_exit_code(0);
    let missing_path = missing.missing_path();
    assert!(matches!(
        run_launcher(missing_path.to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::Launch(_))
    ));
}

#[cfg(unix)]
#[test]
fn terminate_matches_accepts_documented_statuses_only() {
    for code in [0, 1, 128] {
        let fixture = Fixture::with_exit_code(code);
        assert!(terminate_matches(fixture.path().to_str().unwrap(), &[]).is_ok());
    }

    let rejected = Fixture::with_exit_code(2);
    assert!(matches!(
        terminate_matches(rejected.path().to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::TerminateFailed(Some(2)))
    ));

    let missing = Fixture::with_exit_code(0);
    let missing_path = missing.missing_path();
    assert!(matches!(
        terminate_matches(missing_path.to_str().unwrap(), &[]),
        Err(ClaudeDesktopError::Terminate(_))
    ));
}

#[test]
fn tasklist_parser_handles_csv_names_and_localized_empty_output() {
    assert!(tasklist_reports_desktop(
        br#""Claude.exe",1234,Console,1,42 K"#
    ));
    assert!(tasklist_reports_desktop(
        br#""CLAUDE.EXE",1234,Console,1,42 K"#
    ));
    assert!(!tasklist_reports_desktop(
        br#""Other.exe",1234,Console,1,42 K"#
    ));
    assert!(!tasklist_reports_desktop(
        "Información no disponible".as_bytes()
    ));
}

#[cfg(unix)]
#[test]
fn explicit_executables_require_a_file_and_executable_permissions() {
    let executable = Fixture::with_exit_code(0);
    assert!(is_executable_file(executable.path()));
    assert_eq!(
        find_executable(executable.path().to_str().unwrap()),
        Some(executable.path().to_path_buf())
    );

    let directory = tempfile::tempdir().expect("directory fixture");
    assert!(!is_executable_file(directory.path()));
    assert!(find_executable(directory.path().to_str().unwrap()).is_none());

    let non_executable = Fixture::with_exit_code(0);
    fs::set_permissions(non_executable.path(), fs::Permissions::from_mode(0o600))
        .expect("non-executable permissions");
    assert!(!is_executable_file(non_executable.path()));
    assert!(matches!(
        SystemDesktopProcess::new(
            DesktopPlatform::Macos,
            Some(non_executable.path().to_path_buf())
        )
        .ensure_available(),
        Err(ClaudeDesktopError::AppNotFound { .. })
    ));
}

#[cfg(unix)]
#[test]
fn explicit_executable_availability_accepts_a_matching_file() {
    let executable = Fixture::with_exit_code(0);
    assert!(matches!(
        SystemDesktopProcess::new(
            DesktopPlatform::Linux,
            Some(executable.path().to_path_buf())
        )
        .ensure_available(),
        Ok(())
    ));
}

#[test]
fn find_versioned_windows_app_uses_the_latest_matching_fixture() {
    let root = tempfile::tempdir().expect("windows app root");
    for version in ["app-1", "app-2"] {
        let directory = root.path().join(version);
        fs::create_dir_all(&directory).expect("version directory");
        File::create(directory.join("Claude.exe")).expect("versioned executable");
    }
    fs::create_dir_all(root.path().join("not-an-app")).expect("ignored directory");
    File::create(root.path().join("not-an-app/Claude.exe")).expect("ignored executable");

    assert_eq!(
        find_versioned_windows_app(root.path()),
        Some(root.path().join("app-2/Claude.exe"))
    );
    assert!(find_versioned_windows_app(&root.path().join("missing")).is_none());
}
