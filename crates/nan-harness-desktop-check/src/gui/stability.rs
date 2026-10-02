//! Closed observations explain acquisition failures without exposing window metadata.

use crate::native::Window;
use serde::Serialize;
use std::io::Write as _;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stability {
    observations: u16,
    candidates_present: u16,
    candidates_absent: u16,
    identity_changes: u16,
    bounds_changes: u16,
    name_changes: u16,
    stable_pairs: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_window_state: Option<&'static str>,
}

impl Stability {
    pub(super) fn observe(&mut self, current: Option<&Window>, previous: Option<&Window>) {
        if self.observations >= 512 {
            return;
        }
        self.observations += 1;
        let Some(current) = current else {
            self.candidates_absent += 1;
            return;
        };
        self.candidates_present += 1;
        if let Some(previous) = previous {
            self.identity_changes +=
                u16::from(current.id != previous.id || current.pid != previous.pid);
            self.bounds_changes += u16::from(current.bounds != previous.bounds);
            self.name_changes += u16::from(current.name != previous.name);
            self.stable_pairs += u16::from(current == previous);
        }
    }

    pub(super) fn save_failure(
        &mut self,
        native: &crate::native::Native,
        snapshot: &crate::native::Snapshot,
        previous: Option<&Window>,
    ) {
        #[cfg(windows)]
        if let Some(window) = previous {
            let observed = snapshot
                .windows
                .iter()
                .find(|candidate| candidate.id == window.id && candidate.pid == window.pid);
            self.last_window_state = Some(match observed {
                Some(candidate) if candidate.name != window.name => "candidate-name-mismatch",
                Some(candidate)
                    if candidate.bounds.width < 300 || candidate.bounds.height < 200 =>
                {
                    "candidate-too-small"
                }
                Some(_) => "visible",
                None => native.missing_window_state(window),
            });
        }
        #[cfg(not(windows))]
        let _ = (native, snapshot, previous);
        self.save();
    }

    pub(super) fn save(&self) {
        // This diagnostic never changes the acquisition verdict or authorizes input.
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return;
        }
        let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        if !directory.is_absolute() || !directory.is_dir() || directory.is_symlink() {
            return;
        }
        let mut nonce = [0; 8];
        if getrandom::fill(&mut nonce).is_err() {
            return;
        }
        let path = directory.join(format!("stability-{}.json", u64::from_le_bytes(nonce)));
        let value = serde_json::json!({"schemaVersion": 1, "mechanism": "native-window-stability", "diagnosticsOnly": true, "counts": self});
        if let Ok(bytes) = serde_json::to_vec(&value)
            && let Ok(mut file) = nan_harness_private_fs::open_private_new(&path)
        {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_missing_windows_from_changing_geometry_without_metadata() {
        let mut facts = Stability::default();
        let first = Window {
            id: 10,
            pid: 42,
            bounds: xa11y::Rect {
                x: 0,
                y: 0,
                width: 400,
                height: 300,
            },
            name: "PRIVATE".into(),
            layer: 0,
        };
        let mut moved = first.clone();
        moved.bounds.x = 1;
        facts.observe(None, None);
        facts.observe(Some(&first), None);
        facts.observe(Some(&moved), Some(&first));
        facts.observe(Some(&moved), Some(&moved));
        assert_eq!(
            (
                facts.candidates_absent,
                facts.bounds_changes,
                facts.stable_pairs
            ),
            (1, 1, 1)
        );
        let closed = serde_json::to_string(&facts).unwrap();
        assert!(!closed.contains("PRIVATE"));
        assert_eq!(facts.identity_changes, 0);
        for _ in 0..600 {
            facts.observe(None, None);
        }
        assert_eq!(facts.observations, 512);
    }
}
