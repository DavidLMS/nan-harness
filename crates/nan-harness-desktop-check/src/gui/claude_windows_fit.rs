//! One pre-input fit may reveal the original owned Windows Claude window.
use crate::native::{GuardFailure, Snapshot, Window};

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
