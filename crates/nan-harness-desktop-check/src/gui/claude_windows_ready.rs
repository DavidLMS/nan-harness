//! Final initial geometry binding for the owned Windows Claude startup trial.

use crate::native::{GuardFailure, Snapshot, Window};

#[cfg(windows)]
pub(super) fn policy() -> bool {
    use std::path::PathBuf;
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return false;
    };
    super::qualification_directory::canonical_directory(&directory).is_some()
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        && std::env::var("RUNNER_OS").as_deref() == Ok("Windows")
        && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline")
        && std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() == Ok("private-env")
}

/// Geometry may settle before input; the window identity may never be replaced.
pub(super) fn candidate(
    snapshot: &Snapshot,
    original: &Window,
    eligible: &[&Window],
) -> Result<Window, GuardFailure> {
    let [current] = eligible else {
        return Err(GuardFailure::IdentityMissing);
    };
    if current.id != original.id || current.pid != original.pid || current.name != original.name {
        return Err(GuardFailure::IdentityMissing);
    }
    // The fresh geometry is checked, not the previously unsettled geometry.
    // Foreground HWND, display and occlusion guards remain unchanged.
    snapshot.guard_failure(current)?;
    Ok((*current).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot::parse("FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 636c61756465\n")
            .unwrap()
    }

    #[test]
    fn initial_geometry_can_settle_but_identity_and_occlusion_cannot_change() {
        let mut fresh = snapshot();
        let original = fresh.windows[0].clone();
        fresh.windows[0].bounds.width = 900;
        assert_eq!(
            candidate(&fresh, &original, &[&fresh.windows[0]])
                .unwrap()
                .bounds
                .width,
            900
        );
        assert_eq!(
            candidate(&fresh, &original, &[]),
            Err(GuardFailure::IdentityMissing)
        );
        assert_eq!(
            candidate(&fresh, &original, &[&fresh.windows[0], &fresh.windows[0]]),
            Err(GuardFailure::IdentityMissing)
        );
        for replacement in [
            Window {
                id: 43,
                ..original.clone()
            },
            Window {
                pid: 11,
                ..original.clone()
            },
            Window {
                name: "different".into(),
                ..original.clone()
            },
        ] {
            assert_eq!(
                candidate(&fresh, &replacement, &[&fresh.windows[0]]),
                Err(GuardFailure::IdentityMissing)
            );
        }
        fresh.windows[0].bounds.x = 1900;
        assert_eq!(
            candidate(&fresh, &original, &[&fresh.windows[0]]),
            Err(GuardFailure::OffDisplay)
        );
        let covered = Snapshot::parse("FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 43 11 100 100 800 600 6f74686572\nWIN 42 10 100 100 800 600 636c61756465\n").unwrap();
        assert_eq!(
            candidate(&covered, &original, &[&covered.windows[1]]),
            Err(GuardFailure::Occluded)
        );
    }
}
