use crate::report::Reason;
use std::{
    ffi::OsStr,
    io::{Read as _, Write as _},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use xa11y::Screenshot;
use zeroize::Zeroizing;

const MAX_OUTPUT: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailureCategory {
    InvalidInput,
    Spawn,
    Pipe,
    Output,
    Timeout,
    NonzeroExit,
    WindowChanged,
    WindowQueryRejected,
    SessionUnavailable,
}

impl FailureCategory {
    pub(crate) const fn reason(self) -> Reason {
        match self {
            Self::InvalidInput | Self::Output => Reason::ResponseMismatch,
            Self::Spawn
            | Self::Pipe
            | Self::Timeout
            | Self::NonzeroExit
            | Self::WindowQueryRejected
            | Self::SessionUnavailable => Reason::ActionUnsupported,
            Self::WindowChanged => Reason::WindowChanged,
        }
    }
}

pub(super) fn run(
    executable: &Path,
    argument: &OsStr,
    screenshot: Option<&Screenshot>,
) -> Result<Zeroizing<String>, Reason> {
    run_with_category(executable, argument, screenshot).map_err(FailureCategory::reason)
}

pub(super) fn run_with_category(
    executable: &Path,
    argument: &OsStr,
    screenshot: Option<&Screenshot>,
) -> Result<Zeroizing<String>, FailureCategory> {
    let inventory = screenshot.is_none()
        && (argument == OsStr::new("--windows") || argument == OsStr::new("--windows-absence"));
    for attempt in 0..3 {
        let result = run_once(executable, argument, screenshot, &[]);
        // The X11 helper reads one grabbed server state, so exit 6 means that
        // snapshot was still inconsistent. Discard all of it and repeat only this
        // read-only operation. Never retry input, screenshots, or a guard verdict.
        if inventory && result == Err(FailureCategory::WindowChanged) && attempt < 2 {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        return result;
    }
    unreachable!("the last inventory attempt always returns")
}

#[cfg(target_os = "macos")]
pub(super) fn run_with_category_input(
    executable: &Path,
    argument: &OsStr,
    input: &[u8],
) -> Result<Zeroizing<String>, FailureCategory> {
    // Identity queries are one bounded read and are never retried. The X11
    // inventory retry rule applies only to the existing window snapshot mode.
    run_once(executable, argument, None, input)
}

#[cfg(target_os = "macos")]
pub(super) fn run_fit_until(
    executable: &Path,
    argument: &OsStr,
    deadline: Instant,
) -> Result<Zeroizing<String>, Reason> {
    run_once_until(executable, argument, None, &[], Some(deadline)).map_err(FailureCategory::reason)
}

#[cfg(any(windows, test))]
pub(super) fn run_absence_until(
    executable: &Path,
    deadline: Instant,
) -> Result<Zeroizing<String>, Reason> {
    run_once_until(
        executable,
        OsStr::new("--windows-absence"),
        None,
        &[],
        Some(deadline),
    )
    .map_err(FailureCategory::reason)
}

#[cfg(any(windows, test))]
pub(super) fn run_process_presence_until(
    executable: &Path,
    claude: bool,
    deadline: Instant,
) -> Result<Zeroizing<String>, FailureCategory> {
    let argument = if claude {
        "--claude-process-presence"
    } else {
        "--codex-process-presence"
    };
    run_once_until(
        executable,
        OsStr::new(argument),
        None,
        std::process::id().to_string().as_bytes(),
        Some(deadline),
    )
}

#[cfg(any(windows, test))]
pub(super) fn run_claude_storage_until(
    executable: &Path,
    deadline: Instant,
) -> Result<Zeroizing<String>, FailureCategory> {
    run_once_until(
        executable,
        OsStr::new("--windows-claude-storage"),
        None,
        &[],
        Some(deadline),
    )
}

fn run_once(
    executable: &Path,
    argument: &OsStr,
    screenshot: Option<&Screenshot>,
    input: &[u8],
) -> Result<Zeroizing<String>, FailureCategory> {
    run_once_until(executable, argument, screenshot, input, None)
}

fn run_once_until(
    executable: &Path,
    argument: &OsStr,
    screenshot: Option<&Screenshot>,
    input: &[u8],
    absolute_deadline: Option<Instant>,
) -> Result<Zeroizing<String>, FailureCategory> {
    if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(FailureCategory::Timeout);
    }
    if let Some(image) = screenshot {
        validate_image(image).map_err(|_| FailureCategory::InvalidInput)?;
    }
    let mut command = Command::new(executable);
    command
        .env_clear()
        .arg(argument)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // These are OS-session inputs, not provider credentials or app configuration.
    for name in ["DISPLAY", "XAUTHORITY", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Read-only helper consoles must not disturb the window being observed.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| FailureCategory::Spawn)?;
    let stdin = child.stdin.take().ok_or(FailureCategory::Pipe)?;
    let stdout = child.stdout.take().ok_or(FailureCategory::Pipe)?;
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || write_input(stdin, screenshot, input));
        let reader = scope.spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            stdout
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| FailureCategory::Output)?;
            if bytes.len() as u64 > MAX_OUTPUT {
                return Err(FailureCategory::Output);
            }
            String::from_utf8(bytes.to_vec())
                .map(Zeroizing::new)
                .map_err(|_| FailureCategory::Output)
        });
        let deadline =
            absolute_deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(15));
        let status = loop {
            if absolute_deadline.is_some() && Instant::now() >= deadline {
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) if Instant::now() >= deadline => break None,
                Ok(None) => std::thread::sleep(if absolute_deadline.is_some() {
                    Duration::from_millis(20)
                        .min(deadline.saturating_duration_since(Instant::now()))
                } else {
                    Duration::from_millis(20)
                }),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().map_err(|_| FailureCategory::Pipe)?;
        let output = reader.join().map_err(|_| FailureCategory::Pipe)?;
        if status.is_none() || absolute_deadline.is_some_and(|bound| Instant::now() >= bound) {
            return Err(FailureCategory::Timeout);
        }
        if !status.is_some_and(|status| status.success()) {
            #[cfg(target_os = "macos")]
            if written.is_ok()
                && output.as_ref().is_ok_and(|text| {
                    closed_mac_fit_rejection(
                        argument,
                        absolute_deadline.is_some(),
                        status.and_then(|s| s.code()),
                        text,
                    )
                })
            {
                return output;
            }
            let inventory =
                argument == OsStr::new("--windows") || argument == OsStr::new("--windows-absence");
            return Err(exit_category(
                status.and_then(|status| status.code()),
                inventory,
            ));
        }
        written.map_err(|_| FailureCategory::Pipe)?;
        output
    })
}

#[cfg(target_os = "macos")]
fn closed_mac_fit_rejection(
    argument: &OsStr,
    bounded: bool,
    code: Option<i32>,
    output: &str,
) -> bool {
    bounded
        && code == Some(5)
        && argument
            .to_str()
            .is_some_and(|value| value.starts_with("--fit-window "))
        && super::mac_fit::parse(output).is_some_and(|stage| stage != "completed")
}

fn exit_category(code: Option<i32>, inventory: bool) -> FailureCategory {
    match (inventory, code) {
        (true, Some(5)) => FailureCategory::SessionUnavailable,
        (true, Some(6)) => FailureCategory::WindowChanged,
        (true, Some(7)) => FailureCategory::WindowQueryRejected,
        _ => FailureCategory::NonzeroExit,
    }
}

fn write_input(
    mut stdin: std::process::ChildStdin,
    image: Option<&Screenshot>,
    input: &[u8],
) -> Result<(), FailureCategory> {
    if let Some(image) = image {
        writeln!(stdin, "{} {}", image.width, image.height)
            .and_then(|()| stdin.write_all(&image.pixels))
            .map_err(|_| FailureCategory::Pipe)?;
    } else if !input.is_empty() {
        stdin
            .write_all(input)
            .and_then(|()| stdin.write_all(b"\n"))
            .map_err(|_| FailureCategory::Pipe)?;
    }
    Ok(())
}

pub(super) fn validate_image(image: &Screenshot) -> Result<(), Reason> {
    let pixels = u64::from(image.width) * u64::from(image.height);
    if image.width == 0
        || image.height == 0
        || image.width > 8192
        || image.height > 8192
        || pixels > 16 * 1024 * 1024
        || pixels * 4 != image.pixels.len() as u64
        || !image.scale.is_finite()
        || image.scale <= 0.0
    {
        return Err(Reason::ResponseMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn fit_transport_never_spawns_after_deadline_or_retries_failed_action() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fit");
        let count = directory.path().join("count");
        std::fs::write(&executable, format!("#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nprintf 'attempt\\n' >> '{}'\nexit 5\n", count.display())).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        assert!(
            run_fit_until(&executable, OsStr::new("--fit-window 1 7"), Instant::now()).is_err()
        );
        assert!(!count.exists());
        assert!(
            run_fit_until(
                &executable,
                OsStr::new("--fit-window 1 7"),
                Instant::now() + Duration::from_secs(1)
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(count).unwrap(), "attempt\n");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bounded_fit_retains_closed_rejection_without_turning_nonzero_into_success() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("rejected-fit");
        std::fs::write(
            &executable,
            "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nprintf 'fit-rejected size\\n'\nexit 5\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        assert!(run(&executable, OsStr::new("--fit-window 1 7"), None).is_err());
        let output = run_fit_until(
            &executable,
            OsStr::new("--fit-window 1 7"),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(super::super::mac_fit::parse(&output), Some("size"));
        assert!(!closed_mac_fit_rejection(
            OsStr::new("--windows"),
            true,
            Some(5),
            &output
        ));
        assert!(!closed_mac_fit_rejection(
            OsStr::new("--fit-window 1 7"),
            true,
            Some(5),
            ""
        ));
        assert!(!closed_mac_fit_rejection(
            OsStr::new("--fit-window 1 7"),
            true,
            Some(5),
            "fit-rejected PRIVATE\n"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn presence_command_passes_checker_identity_and_keeps_original_deadline() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("helper");
        std::fs::write(&executable, "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n[ \"$1\" = --claude-process-presence ] || exit 2\nread -r checker\n[ \"$checker\" = \"$PPID\" ] || exit 3\nprintf 'absent\\n'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        assert!(run_process_presence_until(&executable, true, Instant::now()).is_err());
        assert_eq!(
            &*run_process_presence_until(
                &executable,
                true,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
            "absent\n"
        );
        assert!(
            run_process_presence_until(&executable, false, Instant::now() + Duration::from_secs(1))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_absence_never_spawns_expired_or_retries_incomplete_inventory() {
        let (root, executable) = inventory_fixture(1, 6);
        assert!(run_absence_until(&executable, Instant::now()).is_err());
        assert!(!root.path().join("count").exists());
        assert!(run_absence_until(&executable, Instant::now() + Duration::from_secs(1)).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("count")).unwrap(),
            "1\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_absence_kills_only_synthetic_child_and_rejects_late_success() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("synthetic-helper");
        std::fs::write(&executable, "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nprintf 'DISPLAY 0 0 800 600\\n'\nexec /bin/sleep 1\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        assert!(
            run_absence_until(&executable, Instant::now() + Duration::from_millis(50)).is_err()
        );
    }

    #[cfg(unix)]
    fn inventory_fixture(failures: u32, code: i32) -> (tempfile::TempDir, std::path::PathBuf) {
        inventory_fixture_with(
            failures,
            code,
            "FG 42 8\\nDISPLAY 0 0 800 600\\nWIN 8 42 0 0 400 300 -",
        )
    }

    #[cfg(unix)]
    fn inventory_fixture_with(
        failures: u32,
        code: i32,
        snapshot: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("helper");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\n\
                 count_file=\"${{0%/*}}/count\"\ncount=0\n\
                 [ ! -f \"$count_file\" ] || read -r count < \"$count_file\"\n\
                 count=$((count + 1))\nprintf '%s\\n' \"$count\" > \"$count_file\"\n\
                 if [ \"$count\" -le {failures} ]; then\n\
                   printf 'incomplete snapshot\\n'\nexit {code}\nfi\n\
                 printf '{snapshot}\\n'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        (root, executable)
    }

    #[cfg(unix)]
    #[test]
    fn changing_inventory_discards_partial_output_and_restarts_the_complete_read() {
        for argument in ["--windows", "--windows-absence"] {
            let (root, executable) = inventory_fixture(2, 6);
            let output = run_with_category(&executable, OsStr::new(argument), None).unwrap();
            assert_eq!(
                std::fs::read_to_string(root.path().join("count")).unwrap(),
                "3\n"
            );
            assert!(!output.contains("incomplete"));
            let snapshot = super::super::window::Snapshot::parse(&output).unwrap();
            assert_eq!(snapshot.windows.len(), 1);
            assert!(snapshot.require_clear(&snapshot.windows[0]).is_ok());
        }
    }

    #[cfg(unix)]
    #[test]
    fn stale_focus_is_one_complete_snapshot_that_rejects_the_guard() {
        use super::super::window::GuardFailure;
        // A destroyed active-window hint is not a changed inventory: the helper
        // reports exact server focus, here owned by nobody, in a complete read.
        let (root, executable) = inventory_fixture_with(
            0,
            0,
            "FG 0 1\\nDISPLAY 0 0 800 600\\nWIN 8 42 0 0 400 300 -",
        );
        let output = run_with_category(&executable, OsStr::new("--windows"), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("count")).unwrap(),
            "1\n"
        );
        let snapshot = super::super::window::Snapshot::parse(&output).unwrap();
        assert_eq!(
            snapshot.guard_failure(&snapshot.windows[0]),
            Err(GuardFailure::ForegroundChanged)
        );
    }

    #[cfg(unix)]
    #[test]
    fn changing_inventory_stops_after_three_attempts_and_other_operations_never_retry() {
        for (argument, code, count, expected) in [
            ("--windows", 6, "3\n", FailureCategory::WindowChanged),
            ("--windows", 7, "1\n", FailureCategory::WindowQueryRejected),
            ("--windows", 5, "1\n", FailureCategory::SessionUnavailable),
            ("--fit-window", 6, "1\n", FailureCategory::NonzeroExit),
            ("--ocr", 6, "1\n", FailureCategory::NonzeroExit),
        ] {
            let (root, executable) = inventory_fixture(10, code);
            assert_eq!(
                run_with_category(&executable, OsStr::new(argument), None),
                Err(expected)
            );
            assert_eq!(
                std::fs::read_to_string(root.path().join("count")).unwrap(),
                count
            );
        }
    }

    #[test]
    fn native_inventory_exits_have_closed_categories_without_payloads() {
        assert_eq!(
            exit_category(Some(5), true),
            FailureCategory::SessionUnavailable
        );
        assert_eq!(exit_category(Some(6), true), FailureCategory::WindowChanged);
        assert_eq!(
            exit_category(Some(7), true),
            FailureCategory::WindowQueryRejected
        );
        for code in [None, Some(1), Some(6), Some(7), Some(255)] {
            assert_eq!(exit_category(code, false), FailureCategory::NonzeroExit);
        }
        assert_eq!(exit_category(Some(99), true), FailureCategory::NonzeroExit);
    }

    #[test]
    fn malformed_or_excessive_images_never_reach_native_code() {
        for (width, height, scale) in [
            (0, 10, 1.0),
            (8193, 1, 1.0),
            (8192, 8192, 1.0),
            (1, 1, f32::NAN),
        ] {
            assert!(
                validate_image(&Screenshot {
                    width,
                    height,
                    pixels: vec![0; 4],
                    scale
                })
                .is_err()
            );
        }
        assert!(
            validate_image(&Screenshot {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
                scale: 2.0
            })
            .is_ok()
        );
    }
}
