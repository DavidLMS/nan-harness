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
        let result = run_once(executable, argument, screenshot);
        // X11 windows may disappear between enumeration and attribute reads.
        // Discard the entire failed snapshot and repeat only this read-only
        // operation. Never retry input, screenshots, or a completed guard verdict.
        if inventory && result == Err(FailureCategory::WindowChanged) && attempt < 2 {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        return result;
    }
    unreachable!("the last inventory attempt always returns")
}

fn run_once(
    executable: &Path,
    argument: &OsStr,
    screenshot: Option<&Screenshot>,
) -> Result<Zeroizing<String>, FailureCategory> {
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
    let mut child = command.spawn().map_err(|_| FailureCategory::Spawn)?;
    let stdin = child.stdin.take().ok_or(FailureCategory::Pipe)?;
    let stdout = child.stdout.take().ok_or(FailureCategory::Pipe)?;
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || write_image(stdin, screenshot));
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
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) if Instant::now() >= deadline => break None,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().map_err(|_| FailureCategory::Pipe)?;
        let output = reader.join().map_err(|_| FailureCategory::Pipe)?;
        if status.is_none() {
            return Err(FailureCategory::Timeout);
        }
        if !status.is_some_and(|status| status.success()) {
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

fn exit_category(code: Option<i32>, inventory: bool) -> FailureCategory {
    match (inventory, code) {
        (true, Some(5)) => FailureCategory::SessionUnavailable,
        (true, Some(6)) => FailureCategory::WindowChanged,
        (true, Some(7)) => FailureCategory::WindowQueryRejected,
        _ => FailureCategory::NonzeroExit,
    }
}

fn write_image(
    mut stdin: std::process::ChildStdin,
    image: Option<&Screenshot>,
) -> Result<(), FailureCategory> {
    if let Some(image) = image {
        writeln!(stdin, "{} {}", image.width, image.height)
            .and_then(|()| stdin.write_all(&image.pixels))
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

    #[cfg(unix)]
    fn inventory_fixture(failures: u32, code: i32) -> (tempfile::TempDir, std::path::PathBuf) {
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
                 printf 'FG 42 8\\nDISPLAY 0 0 800 600\\nWIN 8 42 0 0 400 300 -\\n'\n"
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
