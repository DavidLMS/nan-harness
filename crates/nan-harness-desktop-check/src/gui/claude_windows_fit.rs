//! One pre-input fit may reveal the original owned Windows Claude window.
use crate::native::{GuardFailure, Snapshot, Window};

/// Passive attachment may lack display containment; it never proves readiness.
pub(super) fn pending_candidate(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
) -> Result<Window, GuardFailure> {
    if let Some(off_display) = candidate(snapshot, original, eligible, false) {
        return Ok(off_display);
    }
    super::claude_windows_ready::candidate(snapshot, original, eligible)
}

pub(super) fn candidate(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
    already_fitted: bool,
) -> Option<Window> {
    let [current] = eligible else { return None };
    if already_fitted
        || current.id != original.id
        || current.pid != original.pid
        || current.name != original.name
        || snapshot.guard_failure(current) != Err(GuardFailure::OffDisplay)
        || snapshot.off_display_relation(current).is_none()
    {
        return None;
    }
    let index = snapshot
        .windows
        .iter()
        .position(|window| window == *current)?;
    // OffDisplay precedes occlusion in the strict guard; prove the latter too.
    if snapshot.windows[..index]
        .iter()
        .any(|window| window.pid == current.pid || overlaps(window, current))
    {
        return None;
    }
    Some((*current).clone())
}

fn overlaps(first: &Window, second: &Window) -> bool {
    let a = first.bounds;
    let b = second.bounds;
    i64::from(a.x) < i64::from(b.x) + i64::from(b.width)
        && i64::from(b.x) < i64::from(a.x) + i64::from(a.width)
        && i64::from(a.y) < i64::from(b.y) + i64::from(b.height)
        && i64::from(b.y) < i64::from(a.y) + i64::from(a.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_requires_unique_original_off_display_unoccluded_foreground() {
        let parse = |text| Snapshot::parse(text).unwrap();
        let valid =
            parse("FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 1800 100 800 600 636c61756465\n");
        let original = valid.windows[0].clone();
        assert!(candidate(&valid, &original, &[&original], false).is_some());
        assert_eq!(
            pending_candidate(&valid, &original, &[&original]).unwrap(),
            original
        );
        assert_eq!(
            super::super::claude_windows_ready::candidate(&valid, &original, &[&original]),
            Err(GuardFailure::OffDisplay)
        );
        assert!(candidate(&valid, &original, &[&original], true).is_none());
        assert!(candidate(&valid, &original, &[], false).is_none());
        assert!(candidate(&valid, &original, &[&original, &original], false).is_none());
        #[cfg(windows)]
        {
            let wrong_window =
                parse("FG 10 99\nDISPLAY 0 0 1920 1080\nWIN 42 10 1800 100 800 600 636c61756465\n");
            assert!(candidate(&wrong_window, &original, &[&original], false).is_none());
        }
        let replacement = Window {
            id: 99,
            ..original.clone()
        };
        assert!(candidate(&valid, &replacement, &[&original], false).is_none());
        for text in [
            "FG 11 43\nDISPLAY 0 0 1920 1080\nWIN 42 10 1800 100 800 600 636c61756465\n",
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 636c61756465\n",
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 43 11 1800 100 800 600 6f74686572\nWIN 42 10 1800 100 800 600 636c61756465\n",
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 43 10 100 100 800 600 636c61756465\nWIN 42 10 1800 100 800 600 636c61756465\n",
        ] {
            let snapshot = parse(text);
            let current = snapshot.windows.last().unwrap();
            assert!(candidate(&snapshot, current, &[current], false).is_none());
        }
    }
}

#[cfg(windows)]
pub(super) fn record(helper_succeeded: bool) {
    use std::io::Write as _;
    if !super::claude_windows_ready::policy() {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
        return;
    };
    let Some(directory) =
        super::qualification_directory::canonical_directory(std::path::Path::new(&directory))
    else {
        return;
    };
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-windows-fit",
        "diagnosticsOnly":true,"phase":"final-ready","fitAttempted":true,"helperSucceeded":helper_succeeded});
    if let Ok(bytes) = serde_json::to_vec(&value) {
        if let Ok(mut file) = nan_harness_private_fs::open_private_new(
            directory.join(format!("claude-fit-{}.json", u64::from_le_bytes(nonce))),
        ) {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    }
}
