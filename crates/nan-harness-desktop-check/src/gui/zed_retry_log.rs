//! Passive pinned-source counters; log contents never establish action authority.
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

const LIMIT: u64 = 262_144;
#[cfg(target_os = "linux")]
const NO_FOLLOW: i32 = 0x20000 | 0x800; // O_NOFOLLOW | O_NONBLOCK
#[cfg(not(target_os = "linux"))]
const NO_FOLLOW: i32 = 0x100 | 0x4; // O_NOFOLLOW | O_NONBLOCK

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Receipt {
    status: &'static str,
    prior_turn_observed: bool,
    #[serde(flatten)]
    counts: Counts,
}
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    session_found: u8,
    session_missing: u8,
    message_totals: u8,
    turn_started: u8,
    turn_completed: u8,
    turn_failed: u8,
    turn_cancelled: u8,
}
impl Counts {
    fn counter(&mut self, index: usize) -> &mut u8 {
        match index {
            0 => &mut self.session_found,
            1 => &mut self.session_missing,
            2 => &mut self.message_totals,
            3 => &mut self.turn_started,
            4 => &mut self.turn_completed,
            5 => &mut self.turn_failed,
            _ => &mut self.turn_cancelled,
        }
    }
}
impl Receipt {
    fn failed(status: &'static str) -> Self {
        Self {
            status,
            prior_turn_observed: false,
            counts: Counts::default(),
        }
    }
}

pub(super) struct Capture {
    file: File,
    path: PathBuf,
    identity: (u64, u64),
    offset: u64,
    prior_turn_observed: bool,
}
impl Capture {
    pub(super) fn begin(path: &Path) -> Result<Self, Receipt> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW)
            .open(path)
            .map_err(|error| {
                Receipt::failed(if error.kind() == std::io::ErrorKind::NotFound {
                    "missing"
                } else {
                    "unavailable"
                })
            })?;
        let meta = file
            .metadata()
            .map_err(|_| Receipt::failed("unavailable"))?;
        if !meta.is_file() {
            return Err(Receipt::failed("unavailable"));
        }
        if meta.len() > LIMIT {
            return Err(Receipt::failed("limit"));
        }
        // A stopped classifier stays stopped: an earlier limit must not become
        // an apparently complete, empty interval when Retry begins later.
        let mut baseline = zeroize::Zeroizing::new(Vec::new());
        (&mut file)
            .take(meta.len())
            .read_to_end(&mut baseline)
            .map_err(|_| Receipt::failed("unavailable"))?;
        if baseline.len() as u64 != meta.len() {
            return Err(Receipt::failed("truncated"));
        }
        let previous = classify(&baseline);
        if previous.status != "complete" {
            return Err(previous);
        }
        let prior_turn_observed = previous.counts.session_found > 0
            && previous.counts.turn_started > 0
            && previous.counts.turn_completed > 0;
        Ok(Self {
            file,
            path: path.to_owned(),
            identity: (meta.dev(), meta.ino()),
            offset: meta.len(),
            prior_turn_observed,
        })
    }
    pub(super) fn finish(mut self) -> Receipt {
        let Ok(meta) = std::fs::symlink_metadata(&self.path) else {
            return Receipt::failed("unavailable");
        };
        if !meta.is_file() || (meta.dev(), meta.ino()) != self.identity {
            return Receipt::failed("rotated");
        }
        let end = meta.len();
        if end < self.offset {
            return Receipt::failed("truncated");
        }
        if end - self.offset > LIMIT {
            return Receipt::failed("limit");
        }
        if self.file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Receipt::failed("unavailable");
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        if (&mut self.file)
            .take(end - self.offset)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Receipt::failed("unavailable");
        }
        if bytes.len() as u64 != end - self.offset {
            return Receipt::failed("truncated");
        }
        let Ok(current) = std::fs::symlink_metadata(&self.path) else {
            return Receipt::failed("unavailable");
        };
        let Ok(held) = self.file.metadata() else {
            return Receipt::failed("unavailable");
        };
        if !current.is_file() || (current.dev(), current.ino()) != self.identity {
            return Receipt::failed("rotated");
        }
        if held.len() < end || current.len() < end {
            return Receipt::failed("truncated");
        }
        let mut receipt = classify(&bytes);
        receipt.prior_turn_observed = self.prior_turn_observed;
        receipt
    }
}

fn classify(bytes: &[u8]) -> Receipt {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Receipt::failed("truncated");
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Receipt::failed("unavailable");
    };
    let mut receipt = Receipt::failed("complete");
    let events = [
        "session-found",
        "session-missing",
        "message-totals",
        "turn-started",
        "turn-completed",
        "turn-failed",
        "turn-cancelled",
    ];
    for line in text.lines() {
        if line == "limit" {
            return Receipt::failed("limit");
        }
        let Some(index) = events.iter().position(|event| *event == line) else {
            return Receipt::failed("unavailable");
        };
        let slot = receipt.counts.counter(index);
        let Some(count) = slot.checked_add(1) else {
            return Receipt::failed("limit");
        };
        *slot = count;
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn closed_events_only_and_limits_are_distinguished() {
        let receipt = classify(b"session-found\nmessage-totals\n");
        assert_eq!(receipt.counts.session_found, 1);
        assert_eq!(receipt.counts.message_totals, 1);
        assert_eq!(classify(b"PRIVATE\n").status, "unavailable");
        assert_eq!(classify(b"limit\n").status, "limit");
        assert_eq!(classify(b"unfinished").status, "truncated");
    }
    #[test]
    fn limit_before_retry_cannot_be_reported_as_empty_success() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("events");
        std::fs::write(&path, b"session-found\nlimit\n").unwrap();
        assert_eq!(Capture::begin(&path).err().unwrap().status, "limit");
    }
    #[test]
    fn baseline_requires_a_completed_known_turn_and_never_counts_it_in_interval() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("events");
        for (prefix, expected) in [
            (b"".as_slice(), false),
            (b"session-found\nturn-started\n".as_slice(), false),
            (
                b"session-found\nturn-started\nturn-completed\n".as_slice(),
                true,
            ),
        ] {
            std::fs::write(&path, prefix).unwrap();
            let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
            OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"message-totals\n")
                .unwrap();
            let receipt = capture.finish();
            assert_eq!(receipt.status, "complete");
            assert_eq!(receipt.prior_turn_observed, expected);
            assert_eq!(receipt.counts.session_found, 0);
            assert_eq!(receipt.counts.message_totals, 1);
        }
    }
    #[test]
    fn original_file_interval_excludes_old_records_and_rejects_replacement() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path();
        let path = dir.join("Zed.log");
        std::fs::write(&path, b"session-found\n").unwrap();
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        let receipt = capture.finish();
        assert_eq!(receipt.counts, Counts::default());
        assert!(!receipt.prior_turn_observed);
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"message-totals\n")
            .unwrap();
        assert_eq!(capture.finish().counts.message_totals, 1);
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        std::fs::rename(&path, dir.join("old")).unwrap();
        std::fs::write(&path, b"").unwrap();
        assert_eq!(capture.finish().status, "rotated");
        std::fs::write(&path, b"session-found\nmessage-totals\n").unwrap();
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        std::fs::write(&path, b"").unwrap();
        assert_eq!(capture.finish().status, "truncated");
    }
}
