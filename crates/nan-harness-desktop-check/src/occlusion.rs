//! Transient, closed occluder classification for the wave10 Zed macOS
//! trust-dialog-discovery blocker.
//!
//! This is an experiment-only diagnostic. It is emitted only when the native
//! window guard would reject the owned target window as `WindowOccluded`, and
//! it reports the intersecting window(s) ahead of that target using a small
//! enum of closed classes plus bounded numeric geometry. It never writes a
//! process name, window title, path, screenshot, prompt, or raw inventory.
//! The public `report.json` schema is not changed; this lives in a separate
//! file that is independently validated before it may be uploaded.

use serde::{Deserialize, Serialize};
use std::io::{self, Read as _};
use std::path::Path;

pub(crate) const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;
const MAX_OCCLUDERS: usize = 1024;
const MIN_LAYER: i32 = -200;
const MAX_LAYER: i32 = 200;
/// Largest overlap area the bounded coordinate contract can produce
/// (`65536 * 65536`).
const MAX_OVERLAP_AREA: u64 = 65536 * 65536;

/// Closed classification of a window owner that intersects the owned target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OccluderClass {
    /// The macOS Dock.
    Dock,
    /// Menu-bar / system UI windows (`SystemUIServer`, `ControlCenter`).
    SystemUi,
    /// The WindowServer-owned drawing surface (`WindowServer`).
    WindowServer,
    /// A window owner not matched by the closed allowlist (any external app).
    Other,
    /// The owner is unknown (no bound process name).
    Unknown,
}

/// Classify a decoded process name into a closed enum using a fixed allowlist.
/// Unrecognized names resolve to `Other`; empty names resolve to `Unknown`.
/// The raw string is never serialized.
#[must_use]
pub(crate) fn classify_owner(name: &str) -> OccluderClass {
    match name {
        "WindowServer" => OccluderClass::WindowServer,
        "Dock" => OccluderClass::Dock,
        "SystemUIServer" | "ControlCenter" | "NotificationCenter" | "loginwindow" => {
            OccluderClass::SystemUi
        }
        "" => OccluderClass::Unknown,
        _ => OccluderClass::Other,
    }
}

/// One intersecting window ahead of the owned target, in closed form only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Occluder {
    pub(crate) class: OccluderClass,
    /// Whether the window is owned by the same process as the target.
    pub(crate) same_process: bool,
    /// CoreGraphics window level (`kCGWindowLayer`); 0 when not reported.
    pub(crate) layer: i32,
    /// Intersection area with the target, in squared units (bounded).
    pub(crate) overlap_area: u64,
    /// Fraction of the target area covered, in per-mille (0..=1000).
    pub(crate) overlap_per_mille: u16,
}

/// The complete closed occlusion diagnostic for one rejected guard snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OcclusionDiagnostic {
    pub(crate) schema_version: u8,
    pub(crate) occluder_count: usize,
    pub(crate) occluders: Vec<Occluder>,
}

impl OcclusionDiagnostic {
    #[must_use]
    pub(crate) fn new(occluders: Vec<Occluder>) -> Self {
        let occluder_count = occluders.len();
        Self {
            schema_version: 1,
            occluder_count,
            occluders,
        }
    }

    /// Validate the closed schema and privacy contract.
    ///
    /// # Errors
    /// Rejects an oversized, malformed, over-bounded or unallowlisted document.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, OcclusionError> {
        if bytes.len() > MAX_DIAGNOSTIC_BYTES {
            return Err(OcclusionError::TooLarge);
        }
        let diagnostic: Self = serde_json::from_slice(bytes).map_err(|_| OcclusionError::Schema)?;
        diagnostic.validate()?;
        Ok(diagnostic)
    }

    /// Read a bounded, allowlisted diagnostic without exposing parser payloads.
    ///
    /// # Errors
    /// Fails on invalid evidence, unknown fields or an oversized document.
    pub(crate) fn read(path: &Path) -> Result<(Self, String), OcclusionError> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_DIAGNOSTIC_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let diagnostic = Self::parse(&bytes)?;
        Ok((diagnostic, digest(&bytes)))
    }

    fn validate(&self) -> Result<(), OcclusionError> {
        if self.schema_version != 1
            || self.occluder_count != self.occluders.len()
            || self.occluders.len() > MAX_OCCLUDERS
            || (self.occluder_count == 0 && !self.occluders.is_empty())
        {
            return Err(OcclusionError::Schema);
        }
        for occluder in &self.occluders {
            if occluder.overlap_per_mille > 1000
                || occluder.overlap_area > MAX_OVERLAP_AREA
                || occluder.layer < MIN_LAYER
                || occluder.layer > MAX_LAYER
            {
                return Err(OcclusionError::Schema);
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OcclusionError {
    #[error("occlusion diagnostic cannot be read")]
    Io(#[from] io::Error),
    #[error("occlusion diagnostic is too large or out of range")]
    TooLarge,
    #[error("occlusion diagnostic does not match the closed schema")]
    Schema,
}

#[must_use]
pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        })
}

/// Persist the closed occluder diagnostic when the probe is asked to record
/// it. The destination comes from the `NAN_DESKTOP_OCCLUSION_DIAGNOSTIC`
/// variable, which the transient wave10 workflow sets to a closed file next to
/// the public report. When the variable is absent (the normal product path) or
/// the path is unwritable this is a silent no-op so the guard verdict and the
/// public report are never altered.
pub(crate) fn emit(diagnostic: &OcclusionDiagnostic) {
    // The normal product path has no diagnostic variable set, so this is a
    // no-op there; only the transient wave10 workflow names a destination.
    let Some(path) = std::env::var_os("NAN_DESKTOP_OCCLUSION_DIAGNOSTIC") else {
        return;
    };
    emit_to(diagnostic, &std::path::PathBuf::from(path));
}

/// Write a validated diagnostic to a freshly created private file.
fn emit_to(diagnostic: &OcclusionDiagnostic, path: &Path) {
    if !path.is_absolute() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec_pretty(diagnostic) else {
        return;
    };
    // Only a bounded, validated document is admissible; never publish a
    // partial or malformed file.
    if OcclusionDiagnostic::parse(&bytes).is_err() {
        return;
    }
    // Exclusively create a hardened private file. `open_private_new` fails
    // closed on a preexisting destination, a symbolic link, or an otherwise
    // unowned path, so the first rejected snapshot is preserved and a later
    // failure can never replace earlier evidence.
    let Ok(mut file) = nan_harness_private_fs::open_private_new(path) else {
        return;
    };
    let result = (|| -> io::Result<()> {
        use std::io::Write as _;
        file.write_all(&bytes)?;
        file.sync_all()
    })();
    if result.is_err() {
        // Never leave a partial file behind that could be mistaken for a
        // complete, upload-eligible diagnostic.
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_owner_resolves_only_closed_allowlist_names() {
        assert_eq!(classify_owner("WindowServer"), OccluderClass::WindowServer);
        assert_eq!(classify_owner("Dock"), OccluderClass::Dock);
        assert_eq!(classify_owner("ControlCenter"), OccluderClass::SystemUi);
        assert_eq!(classify_owner("SystemUIServer"), OccluderClass::SystemUi);
        assert_eq!(classify_owner(""), OccluderClass::Unknown);
        assert_eq!(classify_owner("SomeExternalApp"), OccluderClass::Other);
    }

    #[test]
    fn closed_round_trip_binds_the_exact_closed_fields() {
        let diag = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::WindowServer,
            same_process: false,
            layer: -1,
            overlap_area: 123_456,
            overlap_per_mille: 250,
        }]);
        let bytes = serde_json::to_vec(&diag).unwrap();
        let decoded = OcclusionDiagnostic::parse(&bytes).unwrap();
        assert_eq!(decoded, diag);
    }

    #[test]
    fn diagnostic_rejects_unauthorized_fields_and_overflow_bounds() {
        let diag = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::WindowServer,
            same_process: false,
            layer: -1,
            overlap_area: 1,
            overlap_per_mille: 10,
        }]);
        let mut json = serde_json::to_value(&diag).unwrap();
        json["processName"] = "private-process".into();
        assert!(matches!(
            OcclusionDiagnostic::parse(&serde_json::to_vec(&json).unwrap()),
            Err(OcclusionError::Schema)
        ));
        json = serde_json::to_value(&diag).unwrap();
        json["occluders"][0]["layer"] = serde_json::json!(99999);
        assert!(matches!(
            OcclusionDiagnostic::parse(&serde_json::to_vec(&json).unwrap()),
            Err(OcclusionError::Schema)
        ));
        json = serde_json::to_value(&diag).unwrap();
        json["occluders"][0]["overlapPerMille"] = serde_json::json!(2000);
        assert!(matches!(
            OcclusionDiagnostic::parse(&serde_json::to_vec(&json).unwrap()),
            Err(OcclusionError::Schema)
        ));
        json = serde_json::to_value(&diag).unwrap();
        json["occluders"][0]["overlapArea"] = serde_json::json!(u64::from(MAX_OVERLAP_AREA) + 1);
        assert!(matches!(
            OcclusionDiagnostic::parse(&serde_json::to_vec(&json).unwrap()),
            Err(OcclusionError::Schema)
        ));
        assert!(matches!(
            OcclusionDiagnostic::parse(&vec![b' '; MAX_DIAGNOSTIC_BYTES + 1]),
            Err(OcclusionError::TooLarge)
        ));
    }

    #[test]
    fn diagnostic_rejects_arbitrary_class_enum_strings() {
        let diag = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::Dock,
            same_process: true,
            layer: 0,
            overlap_area: 1,
            overlap_per_mille: 1,
        }]);
        let mut json = serde_json::to_value(&diag).unwrap();
        json["occluders"][0]["class"] = serde_json::json!("a-private-app-name");
        assert!(matches!(
            OcclusionDiagnostic::parse(&serde_json::to_vec(&json).unwrap()),
            Err(OcclusionError::Schema)
        ));
    }

    #[test]
    fn read_bounds_the_file_before_allocation() {
        use std::io::Write as _;
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("diagnostic.json");
        // A valid document round-trips through the bounded read.
        let diag = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::Dock,
            same_process: true,
            layer: 3,
            overlap_area: 1,
            overlap_per_mille: 1,
        }]);
        let mut file = std::fs::File::create(&path).expect("file should be created");
        file.write_all(&serde_json::to_vec(&diag).unwrap()).unwrap();
        let (decoded, _) = OcclusionDiagnostic::read(&path).expect("bounded read should succeed");
        assert_eq!(decoded, diag);
        // An oversized document is rejected before it is read into a large buffer.
        let big = directory.path().join("big.json");
        let mut file = std::fs::File::create(&big).expect("file should be created");
        file.write_all(&vec![b' '; MAX_DIAGNOSTIC_BYTES + 1])
            .unwrap();
        assert!(matches!(
            OcclusionDiagnostic::read(&big),
            Err(OcclusionError::TooLarge)
        ));
    }

    #[test]
    fn emit_preserves_the_first_snapshot_and_never_writes_into_an_existing_file() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("diagnostic.json");
        // A relative destination is not an experiment-owned absolute path.
        emit_to(
            &OcclusionDiagnostic::new(vec![]),
            Path::new("diagnostic.json"),
        );
        assert!(!path.exists());

        let diag = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::Dock,
            same_process: false,
            layer: 1,
            overlap_area: 1,
            overlap_per_mille: 1,
        }]);
        emit_to(&diag, &path);
        assert!(path.exists());
        let first = std::fs::read(&path).unwrap();
        assert!(OcclusionDiagnostic::parse(&first).is_ok());

        // A second snapshot must not replace the first rejected snapshot.
        let other = OcclusionDiagnostic::new(vec![Occluder {
            class: OccluderClass::WindowServer,
            same_process: false,
            layer: -1,
            overlap_area: 2,
            overlap_per_mille: 2,
        }]);
        emit_to(&other, &path);
        let second = std::fs::read(&path).unwrap();
        assert_eq!(first, second, "the first snapshot must be preserved");
    }
}
