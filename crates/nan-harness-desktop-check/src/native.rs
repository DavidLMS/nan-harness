//! Private, offline native helper. No captured pixels or recognized text are persisted.

mod image;
mod ocr;
mod process;
mod window;

pub(crate) use image::prepare_ocr_image;
pub(crate) use ocr::Page;
pub(crate) use process::FailureCategory;
pub(crate) use window::{ForegroundRelation, GuardFailure, Snapshot, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FitFailureStage {
    Request,
    IdentityRead,
    IdentityMismatch,
    ForegroundRead,
    ForegroundMismatch,
    MonitorRead,
    WorkareaRead,
    WindowRead,
    WorkareaInvalid,
    IdentityChanged,
    ForegroundChanged,
    Resize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FitFailure {
    pub(crate) stage: FitFailureStage,
    pub(crate) os_error: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FitWindowError {
    Transport(FailureCategory),
    Diagnostic(FitFailure),
}

impl FitFailure {
    fn parse(output: &str) -> Result<Self, Reason> {
        let mut fields = output.lines().flat_map(str::split_whitespace);
        if fields.next() != Some("FIT_FAILURE") {
            return Err(Reason::ActionUnsupported);
        }
        let stage = match fields.next() {
            Some("request") => FitFailureStage::Request,
            Some("identity-read") => FitFailureStage::IdentityRead,
            Some("identity-mismatch") => FitFailureStage::IdentityMismatch,
            Some("foreground-read") => FitFailureStage::ForegroundRead,
            Some("foreground-mismatch") => FitFailureStage::ForegroundMismatch,
            Some("monitor-read") => FitFailureStage::MonitorRead,
            Some("workarea-read") => FitFailureStage::WorkareaRead,
            Some("window-read") => FitFailureStage::WindowRead,
            Some("workarea-invalid") => FitFailureStage::WorkareaInvalid,
            Some("identity-changed") => FitFailureStage::IdentityChanged,
            Some("foreground-changed") => FitFailureStage::ForegroundChanged,
            Some("resize") => FitFailureStage::Resize,
            _ => return Err(Reason::ActionUnsupported),
        };
        let os_error = fields
            .next()
            .map(|value| value.parse().map_err(|_| Reason::ActionUnsupported))
            .transpose()?;
        if fields.next().is_some()
            || (os_error.is_some()
                && !matches!(
                    stage,
                    FitFailureStage::IdentityRead
                        | FitFailureStage::MonitorRead
                        | FitFailureStage::WorkareaRead
                        | FitFailureStage::WindowRead
                        | FitFailureStage::Resize
                ))
        {
            return Err(Reason::ActionUnsupported);
        }
        Ok(Self { stage, os_error })
    }
}

use crate::report::Reason;
use nan_harness_private_fs::{create_private_dir_all, open_private_new};
use std::io::Write as _;
use std::path::PathBuf;
use xa11y::Screenshot;

pub(crate) struct Native {
    directory: tempfile::TempDir,
    executable: PathBuf,
}

impl Native {
    pub(crate) fn new() -> Result<Self, Reason> {
        let directory = tempfile::Builder::new()
            .prefix("nanh-desktop-native-")
            .tempdir()
            .map_err(|_| Reason::IsolationUnavailable)?;
        create_private_dir_all(directory.path()).map_err(|_| Reason::IsolationUnavailable)?;
        let executable = directory.path().join(if cfg!(windows) {
            "helper.exe"
        } else {
            "helper"
        });
        open_private_new(&executable)
            .and_then(|mut file| file.write_all(include_bytes!(env!("NAN_DESKTOP_NATIVE_HELPER"))))
            .map_err(|_| Reason::IsolationUnavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Reason::IsolationUnavailable)?;
        }
        open_private_new(&directory.path().join("eng.traineddata"))
            .and_then(|mut file| file.write_all(include_bytes!(env!("NAN_DESKTOP_OCR_MODEL"))))
            .map_err(|_| Reason::IsolationUnavailable)?;
        Ok(Self {
            directory,
            executable,
        })
    }

    pub(crate) fn windows(&self) -> Result<Snapshot, Reason> {
        self.windows_with_category()
            .map_err(FailureCategory::reason)
    }

    pub(crate) fn windows_with_category(&self) -> Result<Snapshot, FailureCategory> {
        let output =
            process::run_with_category(&self.executable, std::ffi::OsStr::new("--windows"), None)?;
        Snapshot::parse(&output).map_err(|_| FailureCategory::Pipe)
    }

    pub(crate) fn windows_for_absence(&self) -> Result<Vec<Window>, Reason> {
        // Absence needs complete window enumeration, not a focused application.
        // Return only windows so this inventory cannot certify an input/capture guard.
        let output = process::run(
            &self.executable,
            std::ffi::OsStr::new("--windows-absence"),
            None,
        )?;
        Ok(Snapshot::parse(&output)?.windows)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn activate_owned_window(&self, window: &Window) -> Result<(), FailureCategory> {
        let argument = format!("--activate-window {} {}", window.id, window.pid);
        process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None)
            .map(|_| ())
    }

    #[cfg(windows)]
    pub(crate) fn fit_owned_window(&self, window: &Window) -> Result<(), FitWindowError> {
        let argument = format!("--fit-window {} {}", window.id, window.pid);
        let output =
            process::run_with_category(&self.executable, std::ffi::OsStr::new(&argument), None)
                .map_err(FitWindowError::Transport)?;
        if output.trim().is_empty() {
            return Ok(());
        }
        Err(FitWindowError::Diagnostic(
            FitFailure::parse(&output).unwrap_or(FitFailure {
                stage: FitFailureStage::Request,
                os_error: None,
            }),
        ))
    }

    pub(crate) fn recognize(&self, screenshot: &Screenshot) -> Result<Page, Reason> {
        let output = process::run(
            &self.executable,
            self.directory.path().as_os_str(),
            Some(screenshot),
        )?;
        Page::parse(&output, screenshot.width, screenshot.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_window_fitting_rejects_missing_identity_without_changing_windows() {
        let native = Native::new().unwrap();
        assert!(
            process::run(
                &native.executable,
                std::ffi::OsStr::new("--fit-window 0 0"),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn fit_failure_protocol_is_closed_and_keeps_observed_errors_only() {
        assert_eq!(
            FitFailure::parse("FIT_FAILURE resize 1400\n")
                .unwrap()
                .os_error,
            Some(1400)
        );
        assert_eq!(
            FitFailure::parse("FIT_FAILURE identity-mismatch\n")
                .unwrap()
                .stage,
            FitFailureStage::IdentityMismatch
        );
        for invalid in [
            "FIT_FAILURE resize private\n",
            "FIT_FAILURE identity-mismatch 5\n",
            "FIT_FAILURE resize 1 extra\n",
            "FIT_FAILURE unknown\n",
            "FIT_FAILURE resize 4294967296\n",
        ] {
            assert!(FitFailure::parse(invalid).is_err());
        }
    }

    #[test]
    fn native_window_activation_rejects_invalid_identity_without_activation() {
        let native = Native::new().unwrap();
        for request in [
            "--activate-window 0 1",
            "--activate-window 1 0",
            "--activate-window 18446744073709551616 1",
            "--activate-window 4294967296 1",
            "--activate-window -1 1",
            "--activate-window 1 -1",
            "--activate-window 1 4294967296",
            "--activate-window 1 1 trailing",
        ] {
            assert!(process::run(&native.executable, std::ffi::OsStr::new(request), None).is_err());
        }
    }

    #[test]
    fn bundled_helper_runs_without_an_external_ocr_installation() {
        let native = Native::new().unwrap();
        let output =
            process::run(&native.executable, std::ffi::OsStr::new("--version"), None).unwrap();
        assert_eq!(output.trim(), "nanh-desktop-native tesseract-5.5.2");
        assert_eq!(
            crate::report::digest(include_bytes!(env!("NAN_DESKTOP_OCR_MODEL"))),
            "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
        );
    }

    #[test]
    fn helper_failure_categories_are_closed_and_privacy_safe() {
        let categories = [
            (FailureCategory::Spawn, Reason::ActionUnsupported),
            (FailureCategory::Pipe, Reason::ActionUnsupported),
            (FailureCategory::Output, Reason::ResponseMismatch),
            (FailureCategory::Timeout, Reason::ActionUnsupported),
            (FailureCategory::NonzeroExit, Reason::ActionUnsupported),
        ];
        for (category, reason) in categories {
            assert_eq!(category.reason(), reason);
            assert!(matches!(
                category,
                FailureCategory::Spawn
                    | FailureCategory::Pipe
                    | FailureCategory::Output
                    | FailureCategory::Timeout
                    | FailureCategory::NonzeroExit
            ));
        }
    }

    #[test]
    fn bundled_ocr_reads_synthetic_pixels_and_rejects_a_wrong_marker() {
        let glyphs = [
            [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
            [
                "11111", "10000", "10000", "11110", "10000", "10000", "11111",
            ],
            [
                "01111", "10000", "10000", "01110", "00001", "00001", "11110",
            ],
            [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
        ];
        let mut screenshot = Screenshot {
            width: 300,
            height: 100,
            pixels: vec![255; 300 * 100 * 4],
            scale: 1.0,
        };
        for (letter, glyph) in glyphs.iter().enumerate() {
            for (row, line) in glyph.iter().enumerate() {
                for (column, pixel) in line.bytes().enumerate() {
                    if pixel == b'1' {
                        for y in 0..6 {
                            for x in 0..6 {
                                let offset =
                                    ((20 + row * 6 + y) * 300 + 30 + letter * 36 + column * 6 + x)
                                        * 4;
                                screenshot.pixels[offset..offset + 3].fill(0);
                            }
                        }
                    }
                }
            }
        }
        let native = Native::new().unwrap();
        let page = native.recognize(&screenshot).unwrap();
        assert!(page.find_phrase("TEST").is_some());
        assert!(page.find_phrase("WRONG").is_none());
        let scaled = prepare_ocr_image(Screenshot {
            width: screenshot.width,
            height: screenshot.height,
            pixels: screenshot.pixels.clone(),
            scale: screenshot.scale,
        })
        .unwrap();
        let scaled_page = native.recognize(&scaled).unwrap();
        let original_bounds = page.find_phrase("TEST").unwrap();
        let scaled_bounds = scaled_page.find_phrase("TEST").unwrap();
        assert!((scaled_bounds.x / 2 - original_bounds.x).abs() <= 1);
        assert!((scaled_bounds.y / 2 - original_bounds.y).abs() <= 1);
        assert!(scaled_page.find_phrase("WRONG").is_none());
        screenshot.pixels.fill(255);
        assert!(native.recognize(&screenshot).unwrap().words.is_empty());
    }
}
