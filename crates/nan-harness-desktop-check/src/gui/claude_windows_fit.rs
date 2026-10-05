//! One pre-input fit may reveal the original owned Windows Claude window.
use crate::native::{GuardFailure, Snapshot, Window};

#[cfg(any(windows, test))]
pub(super) fn activation_candidate(snapshot: &Snapshot, window: &Window) -> bool {
    snapshot.guard_failure(window) == Err(GuardFailure::ForegroundChanged)
        && snapshot
            .windows
            .iter()
            .filter(|item| item.pid == window.pid)
            .count()
            == 1
        && snapshot.windows.iter().any(|item| item == window)
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum FitRejection {
    AlreadyFitted,
    CandidateCount,
    IdentityMismatch,
    NativeGuard,
    DisplayRelationUnavailable,
    SnapshotIdentityMissing,
    SameProcessAhead,
}

pub(super) fn assess(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
    already_fitted: bool,
) -> Result<Window, FitRejection> {
    if already_fitted {
        return Err(FitRejection::AlreadyFitted);
    }
    let [current] = eligible else {
        return Err(FitRejection::CandidateCount);
    };
    if current.id != original.id || current.pid != original.pid || current.name != original.name {
        return Err(FitRejection::IdentityMismatch);
    }
    if snapshot.guard_failure(current) != Err(GuardFailure::OffDisplay) {
        return Err(FitRejection::NativeGuard);
    }
    if snapshot.off_display_relation(current).is_none() {
        return Err(FitRejection::DisplayRelationUnavailable);
    }
    let index = snapshot
        .windows
        .iter()
        .position(|window| window == *current)
        .ok_or(FitRejection::SnapshotIdentityMissing)?;
    if snapshot.windows[..index]
        .iter()
        .any(|window| window.pid == current.pid)
    {
        return Err(FitRejection::SameProcessAhead);
    }
    // A foreign overlap blocks input, not passive attachment or an owned no-activate fit.
    // The strict readiness guard must prove occlusion absent after the sole fit.
    Ok((*current).clone())
}

pub(super) fn candidate(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
    already_fitted: bool,
) -> Option<Window> {
    assess(snapshot, original, eligible, already_fitted).ok()
}

#[cfg(windows)]
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
    fn initial_activation_requires_one_exact_window_and_foreground_failure() {
        let snapshot = Snapshot::parse(
            "FG 11 43\nDISPLAY 0 0 1920 1080\nWIN 43 11 0 0 1000 800 72756e6e6572\nWIN 42 10 100 100 800 600 636c61756465\n",
        ).unwrap();
        let window = snapshot.windows[1].clone();
        assert!(activation_candidate(&snapshot, &window));
        let mut changed = snapshot.clone();
        changed.windows.push(Window {
            id: 44,
            ..window.clone()
        });
        assert!(!activation_candidate(&changed, &window));
        assert!(!activation_candidate(
            &snapshot,
            &Window {
                id: 99,
                ..window.clone()
            }
        ));
        assert!(!activation_candidate(
            &snapshot,
            &Window {
                name: "replacement".into(),
                ..window.clone()
            }
        ));
        let ready = Snapshot::parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 636c61756465\n",
        )
        .unwrap();
        assert!(!activation_candidate(&ready, &window));
    }
    #[test]
    fn fit_requires_unique_original_off_display_owned_foreground() {
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
        let overlap = parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 43 11 1800 100 800 600 6f74686572\nWIN 42 10 1800 100 800 600 636c61756465\n",
        );
        let current = overlap.windows.last().unwrap();
        assert!(candidate(&overlap, current, &[current], false).is_some());
        assert_eq!(
            pending_candidate(&overlap, current, &[current]),
            Ok(current.clone())
        );
        assert_eq!(
            super::super::claude_windows_ready::candidate(&overlap, current, &[current]),
            Err(GuardFailure::OffDisplay)
        );
        let on_display_overlap = parse(
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 43 11 100 100 800 600 6f74686572\nWIN 42 10 100 100 800 600 636c61756465\n",
        );
        let current = on_display_overlap.windows.last().unwrap();
        assert_eq!(
            super::super::claude_windows_ready::candidate(&on_display_overlap, current, &[current]),
            Err(GuardFailure::Occluded)
        );
        for text in [
            "FG 11 43\nDISPLAY 0 0 1920 1080\nWIN 42 10 1800 100 800 600 636c61756465\n",
            "FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 636c61756465\n",
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
            &directory.join(format!("claude-fit-{}.json", u64::from_le_bytes(nonce))),
        ) {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    }
}

#[cfg(windows)]
pub(super) fn record_rejection(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
    phase: &'static str,
    fitted: bool,
) {
    use std::io::Write as _;
    if !matches!(
        phase,
        "initial-pending" | "pending-attachment" | "final-ready"
    ) || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
    {
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
    let Err(reason) = assess(snapshot, original, eligible, fitted) else {
        return;
    };
    let current = eligible.iter().find(|window| {
        window.id == original.id && window.pid == original.pid && window.name == original.name
    });
    let ahead = current.and_then(|current| {
        snapshot
            .windows
            .iter()
            .position(|window| window == *current)
            .map(|index| (&snapshot.windows[..index], *current))
    });
    let guard = current
        .and_then(|window| snapshot.guard_failure(window).err())
        .map(|failure| match failure {
            GuardFailure::IdentityMissing => "identity-missing",
            GuardFailure::BoundsChanged => "bounds-changed",
            GuardFailure::ForegroundChanged => "foreground-changed",
            GuardFailure::SameProcessWindow => "same-process-window",
            GuardFailure::OffDisplay => "off-display",
            GuardFailure::Occluded => "occluded",
        });
    let same = ahead.map(|(windows, current)| {
        windows
            .iter()
            .filter(|window| window.pid == current.pid)
            .count()
            .min(64)
    });
    let overlap = ahead.map(|(windows, current)| {
        windows
            .iter()
            .filter(|window| overlaps(window, current))
            .count()
            .min(64)
    });
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-windows-fit-rejection","diagnosticsOnly":true,
        "phase":phase,"policyEnabled":super::claude_windows_ready::policy(),"fitAttempted":fitted,
        "sourceComposerReady":(phase == "final-ready").then_some(true),"candidateReason":reason,
        "guardFailure":guard,"eligibleCount":eligible.len().min(64),"sameProcessAheadCount":same,"overlapAheadCount":overlap});
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(&value)
        && let Ok(mut file) = nan_harness_private_fs::open_private_new(&directory.join(format!(
            "claude-fit-rejection-{}.json",
            u64::from_le_bytes(nonce)
        )))
    {
        let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
    }
}
