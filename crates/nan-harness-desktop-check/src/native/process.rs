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
    Timeout,
    NonzeroExit,
}

impl FailureCategory {
    pub(crate) const fn reason(self) -> Reason {
        match self {
            Self::InvalidInput => Reason::ResponseMismatch,
            Self::Spawn | Self::Pipe | Self::Timeout | Self::NonzeroExit => {
                Reason::ActionUnsupported
            }
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
                .map_err(|_| FailureCategory::Pipe)?;
            if bytes.len() as u64 > MAX_OUTPUT {
                return Err(FailureCategory::Pipe);
            }
            String::from_utf8(bytes.to_vec())
                .map(Zeroizing::new)
                .map_err(|_| FailureCategory::Pipe)
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
            return Err(FailureCategory::NonzeroExit);
        }
        written.map_err(|_| FailureCategory::Pipe)?;
        output
    })
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
