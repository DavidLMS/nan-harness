use super::{NativeMcpRoute, marker_write_command, native_calls};
use crate::conformance::inventory::{round_trip_probe, verify_probe_side_effect};
use crate::conformance::registry::harness_registration;
use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
#[cfg(windows)]
use std::path::Path;

#[test]
fn managed_and_native_builders_preserve_the_same_file_and_tool_contracts() {
    let workspace = tempfile::tempdir().unwrap();
    let registration = harness_registration(HarnessKind::MimoCode).unwrap();
    let manifest = registration.manifest().unwrap();
    let probe = round_trip_probe(
        HarnessKind::MimoCode,
        workspace.path(),
        &manifest,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(probe.call.name, "bash");
    assert_eq!(
        probe.call.input["command"],
        marker_write_command("tool-output.txt", "NAN_HARNESS_TOOL_OK")
    );
    assert!(probe.call.result_expected);
    assert!(verify_probe_side_effect(&probe).is_err());
    std::fs::write(workspace.path().join("tool-output.txt"), "wrong marker").unwrap();
    assert!(verify_probe_side_effect(&probe).is_err());
    std::fs::write(
        workspace.path().join("tool-output.txt"),
        "NAN_HARNESS_TOOL_OK",
    )
    .unwrap();
    assert!(verify_probe_side_effect(&probe).is_ok());
    for route in [
        NativeMcpRoute::Direct,
        NativeMcpRoute::Search,
        NativeMcpRoute::Dispatch,
    ] {
        let calls = native_calls(workspace.path(), route);
        let bash = calls.iter().find(|call| call.name == "bash").unwrap();
        assert_eq!(
            bash.input["command"],
            marker_write_command("bash-output.txt", "MIMO_BASH_OK")
        );
        assert!(bash.result_expected);
    }
}

#[cfg(not(windows))]
#[test]
fn unix_mimo_commands_remain_byte_for_byte_unchanged() {
    assert_eq!(
        marker_write_command("tool-output.txt", "NAN_HARNESS_TOOL_OK"),
        "printf NAN_HARNESS_TOOL_OK > tool-output.txt"
    );
    assert_eq!(
        marker_write_command("bash-output.txt", "MIMO_BASH_OK"),
        "printf MIMO_BASH_OK > bash-output.txt"
    );
}

#[cfg(windows)]
fn installed_executable(name: &str) -> std::path::PathBuf {
    let path = std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .expect("native marker proof requires explicit installed interpreter paths");
    assert!(
        path.is_absolute() && path.is_file(),
        "native interpreter must be an absolute file"
    );
    path
}

#[cfg(windows)]
fn failure_category(error: &crate::terminal::TerminalError) -> &'static str {
    use crate::terminal::TerminalError;
    match error {
        TerminalError::Timeout { .. } => "timeout",
        TerminalError::Execute { .. } => "execute",
        TerminalError::MissingOutput { .. } => "missing-output",
        TerminalError::CaptureJoin { .. } => "capture-join",
        TerminalError::Capture { .. } => "capture-read",
        TerminalError::DescendantCleanup { .. } => "descendant-cleanup",
    }
}

#[cfg(windows)]
async fn powershell(
    case: &'static str,
    executable: &Path,
    directory: &Path,
    path: &std::ffi::OsStr,
    command: &str,
) -> crate::terminal::TerminalOutput {
    let prefixed = format!(
        "$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false);{command}"
    );
    let command = crate::terminal::TerminalCommand::new(executable, directory)
        .clear_environment()
        .env("PATH", path)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &prefixed,
        ])
        .timeout(std::time::Duration::from_secs(5));
    crate::conformance::environment::apply(command, directory)
        .unwrap_or_else(|_| panic!("native marker proof case={case} error=environment"))
        .run()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "native marker proof case={case} error={}",
                failure_category(&error)
            )
        })
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires explicit installed PowerShell and Python paths"]
async fn native_windows_marker_command_preserves_contract_without_git() {
    let shell = installed_executable("NAN_MIMO_MARKER_POWERSHELL");
    let python = installed_executable("NAN_MIMO_MARKER_PYTHON");
    let system = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("SYSTEMROOT"))
        .map(std::path::PathBuf::from)
        .expect("native Windows system root is required");
    let paths = [
        shell.parent().unwrap().to_owned(),
        python.parent().unwrap().to_owned(),
        system.join("System32"),
    ];
    let path = std::env::join_paths(&paths).expect("controlled native PATH must serialize");
    let workspace = tempfile::tempdir().unwrap();
    let original = powershell(
        "first-original",
        &shell,
        workspace.path(),
        &path,
        "printf NAN_HARNESS_TOOL_OK > tool-output.txt",
    )
    .await;
    assert!(
        !original.status.success(),
        "original printf fixture must fail without an external printf command"
    );
    assert!(
        !std::fs::read(workspace.path().join("tool-output.txt"))
            .is_ok_and(|bytes| bytes == b"NAN_HARNESS_TOOL_OK"),
        "original fixture must not satisfy the file contract"
    );
    for (case, file, marker) in [
        ("managed", "tool-output.txt", "NAN_HARNESS_TOOL_OK"),
        ("native", "bash-output.txt", "MIMO_BASH_OK"),
    ] {
        let output = powershell(
            case,
            &shell,
            workspace.path(),
            &path,
            &marker_write_command(file, marker),
        )
        .await;
        assert!(
            output.status.success(),
            "portable fixture must execute through native PowerShell"
        );
        assert!(
            std::fs::read(workspace.path().join(file))
                .is_ok_and(|bytes| bytes == marker.as_bytes()),
            "portable fixture must preserve the exact ASCII file contents"
        );
    }
    if let Some(directory) =
        std::env::var_os("NAN_MIMO_MARKER_GIT_USR_BIN").map(std::path::PathBuf::from)
    {
        assert!(
            directory.is_absolute()
                && directory
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
                && directory
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("usr"))
                && directory.join("printf.exe").is_file(),
            "optional positive control requires the Git usr/bin bundle"
        );
        let path = std::env::join_paths(paths.into_iter().chain([directory]))
            .expect("positive control PATH must serialize");
        let output = powershell(
            "git-positive",
            &shell,
            workspace.path(),
            &path,
            "printf OPTIONAL_GIT_PRINTF_OK > git-printf.txt",
        )
        .await;
        assert!(
            output.status.success(),
            "original command must execute with its external printf prerequisite"
        );
        assert!(
            std::fs::read(workspace.path().join("git-printf.txt"))
                .is_ok_and(|bytes| bytes == b"OPTIONAL_GIT_PRINTF_OK"),
            "positive control must preserve its exact file contents"
        );
    }
}
