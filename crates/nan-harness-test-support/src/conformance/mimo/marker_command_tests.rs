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

fn printf_in_path(paths: &[std::path::PathBuf], pathext: &str) -> bool {
    let mut suffixes = ["", ".exe", ".com", ".bat", ".cmd", ".ps1"]
        .map(str::to_owned)
        .into_iter()
        .collect::<BTreeSet<_>>();
    for suffix in pathext
        .split(';')
        .map(str::trim)
        .filter(|suffix| !suffix.is_empty())
    {
        assert!(
            suffix.len() > 1
                && suffix.starts_with('.')
                && suffix[1..].bytes().all(|byte| byte.is_ascii_alphanumeric()),
            "controlled native PATHEXT must contain simple file extensions"
        );
        suffixes.insert(suffix.to_ascii_lowercase());
    }
    for directory in paths {
        assert!(
            directory.is_dir(),
            "controlled native PATH directory must exist"
        );
        for suffix in &suffixes {
            match std::fs::metadata(directory.join(format!("printf{suffix}"))) {
                Ok(metadata) if metadata.is_file() => return true,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!(
                    "controlled printf presence metadata error: {:?}",
                    error.kind()
                ),
            }
        }
    }
    false
}

#[test]
fn printf_absence_checks_every_controlled_directory_and_script_extension() {
    let first = tempfile::tempdir().unwrap();
    let last = tempfile::tempdir().unwrap();
    let paths = [first.path().to_owned(), last.path().to_owned()];
    std::fs::write(last.path().join("unrelated.exe"), b"decoy").unwrap();
    assert!(!printf_in_path(&paths, ".EXE;.CUSTOM"));
    for name in ["printf", "printf.exe", "printf.custom", "printf.ps1"] {
        let file = last.path().join(name);
        std::fs::write(&file, b"synthetic executable or script").unwrap();
        assert!(printf_in_path(&paths, ".EXE;.CUSTOM"));
        std::fs::remove_file(file).unwrap();
    }
    assert!(!printf_in_path(&paths, ".EXE;.CUSTOM"));
}

#[cfg(windows)]
fn private_case_workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("native marker workspace must be created");
    nan_harness_private_fs::create_private_dir_all(workspace.path())
        .unwrap_or_else(|_| panic!("native marker workspace must be private"));
    workspace
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
    // PowerShell can search modules for a missing command. Inspect the optional
    // PATH utility directly so unrelated command discovery is outside this proof.
    let pathext = match std::env::var("PATHEXT") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => String::new(),
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("controlled native PATHEXT must be readable")
        }
    };
    assert!(
        !printf_in_path(&paths, &pathext),
        "portable fixture proof must not contain an optional printf executable or script"
    );
    for (case, file, marker) in [
        ("managed", "tool-output.txt", "NAN_HARNESS_TOOL_OK"),
        ("native", "bash-output.txt", "MIMO_BASH_OK"),
    ] {
        let workspace = private_case_workspace();
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
        let workspace = private_case_workspace();
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
