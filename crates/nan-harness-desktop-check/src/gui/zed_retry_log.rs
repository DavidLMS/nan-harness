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
    #[serde(flatten)]
    counts: Counts,
}
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    session_found: u8,
    session_missing: u8,
    resume_messages: u8,
    ordinary_send: u8,
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
            2 => &mut self.resume_messages,
            3 => &mut self.ordinary_send,
            4 => &mut self.turn_started,
            5 => &mut self.turn_completed,
            6 => &mut self.turn_failed,
            _ => &mut self.turn_cancelled,
        }
    }
}
impl Receipt {
    fn failed(status: &'static str) -> Self {
        Self {
            status,
            counts: Counts::default(),
        }
    }
}

pub(super) struct Capture {
    file: File,
    path: PathBuf,
    identity: (u64, u64),
    offset: u64,
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
        if meta.len() > 0 {
            let mut last = [0_u8];
            file.seek(SeekFrom::Start(meta.len() - 1))
                .map_err(|_| Receipt::failed("unavailable"))?;
            file.read_exact(&mut last)
                .map_err(|_| Receipt::failed("unavailable"))?;
            if last != *b"\n" {
                return Err(Receipt::failed("truncated"));
            }
        }
        Ok(Self {
            file,
            path: path.to_owned(),
            identity: (meta.dev(), meta.ino()),
            offset: meta.len(),
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
        classify(&bytes)
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
    let signatures = [
        ("DEBUG [agent:2267] ", "Found session for: "),
        ("ERROR [agent:2264] ", "Session not found in run_turn: "),
        ("DEBUG [agent::thread:2546] ", "Total messages in thread: "),
        ("DEBUG [agent::thread:2583] ", "Total messages in thread: "),
        (
            "DEBUG [agent::thread:2737] ",
            "Starting agent turn execution",
        ),
        ("DEBUG [agent::thread:2755] ", "Turn execution completed"),
        ("ERROR [agent::thread:2759] ", "Turn execution failed: "),
        (
            "DEBUG [agent::thread:2747] ",
            "Turn was cancelled, skipping cleanup",
        ),
    ];
    for line in text.lines() {
        // Timestamp framing is checked, but logs remain untrusted diagnostic data.
        let Some((stamp, rest)) = line.split_once(' ') else {
            continue;
        };
        if stamp.len() != 25 || stamp.as_bytes().get(10).is_none_or(|c| *c != b'T') {
            continue;
        }
        for (index, (tag, prefix)) in signatures.iter().enumerate() {
            if rest
                .strip_prefix(tag)
                .is_some_and(|message| message.starts_with(prefix))
            {
                let slot = receipt.counts.counter(index);
                let Some(count) = slot.checked_add(1) else {
                    return Receipt::failed("limit");
                };
                *slot = count;
            }
        }
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn counters_discard_private_payload_and_unknown_lines() {
        let receipt = classify(b"2026-10-04T12:00:00+00:00 DEBUG [agent:2267] Found session for: PRIVATE\nspoof DEBUG [agent::thread:2546] Total messages in thread: 7\n2026-10-04T12:00:00+00:00 DEBUG [agent::thread:2546] Total messages in thread: 7\n");
        assert_eq!(receipt.counts.session_found, 1);
        assert_eq!(receipt.counts.resume_messages, 1);
        assert!(!serde_json::to_string(&receipt).unwrap().contains("PRIVATE"));
        assert_eq!(classify(b"unfinished").status, "truncated");
        // Fully framed injected records cannot be authenticated; never input authority.
        assert_eq!(classify(b"2026-10-04T12:00:00+00:00 DEBUG [agent::thread:2583] Total messages in thread: 9\n").counts.ordinary_send, 1);
    }
    #[test]
    fn original_file_interval_excludes_old_records_and_rejects_replacement() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path();
        let path = dir.join("Zed.log");
        std::fs::write(&path, b"old private contents\n").unwrap();
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        assert_eq!(capture.finish().counts, Counts::default());
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        OpenOptions::new().append(true).open(&path).unwrap()
            .write_all(b"2026-10-04T12:00:00+00:00 DEBUG [agent::thread:2546] Total messages in thread: 8\n").unwrap();
        assert_eq!(capture.finish().counts.resume_messages, 1);
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        std::fs::rename(&path, dir.join("old")).unwrap();
        std::fs::write(&path, b"").unwrap();
        assert_eq!(capture.finish().status, "rotated");
        std::fs::write(&path, b"long contents\n").unwrap();
        let capture = Capture::begin(&path).unwrap_or_else(|_| panic!("fixture open"));
        std::fs::write(&path, b"").unwrap();
        assert_eq!(capture.finish().status, "truncated");
    }
}
