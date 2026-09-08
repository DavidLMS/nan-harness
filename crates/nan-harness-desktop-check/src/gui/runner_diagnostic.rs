//! Temporary, explicitly approved synthetic runner diagnostic; remove after review.

use crate::report::Reason;
use std::io::Write as _;

pub(super) fn retain(screenshot: &xa11y::Screenshot, pid: u32) -> Result<(), Reason> {
    let enabled = cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && std::env::var("NAN_DESKTOP_RUNNER_DIAGNOSTIC").as_deref()
            == Ok("approved-synthetic-zed")
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("SELECTED_APP").as_deref() == Ok("zed-desktop")
        && std::env::var_os("NAN_API_KEY").is_none();
    if !enabled {
        return Ok(());
    }
    let root = std::env::var_os("RUNNER_TEMP").ok_or(Reason::IsolationUnavailable)?;
    let directory = std::path::PathBuf::from(root).join("owned-zed-diagnostic");
    nan_harness_private_fs::create_private_dir_all(&directory)
        .map_err(|_| Reason::IsolationUnavailable)?;
    let bytes = screenshot.to_png().map_err(super::map_error)?;
    // Keep only the last guarded frame of each of the three isolated app runs.
    nan_harness_private_fs::open_private_truncate(&directory.join(format!("{pid}.png")))
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|_| Reason::IsolationUnavailable)
}
