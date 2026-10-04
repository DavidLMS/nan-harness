//! Hosted passive source events; payload bytes are never published.
use std::fs::File;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use tokio::process::Command;

pub(super) struct Events {
    file: File,
    line: zeroize::Zeroizing<Vec<u8>>,
    escape: u8,
    discarded: bool,
    count: usize,
    active: bool,
}
impl Events {
    pub(super) fn prepare(command: &Command) -> Option<Self> {
        if !cfg!(target_os = "linux")
            || std::env::var("NANH_ZED_RETRY_EVENTS").as_deref() != Ok("hosted-semantic")
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("Linux")
        {
            return None;
        }
        let args: Vec<_> = command.as_std().get_args().collect();
        let paths: Vec<_> = args
            .windows(2)
            .filter(|pair| pair[0] == "--user-data-dir")
            .collect();
        let [pair] = paths.as_slice() else {
            return None;
        };
        let root = std::path::Path::new(pair[1]);
        if !root.is_absolute()
            || root.file_name()? != "zed"
            || root.parent()?.file_name()? != "profile"
        {
            return None;
        }
        let metadata = std::fs::symlink_metadata(root).ok()?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || std::fs::canonicalize(root).ok().as_deref() != Some(root)
        {
            return None;
        }
        let file =
            nan_harness_private_fs::open_private_new(&root.join("nanh-retry-events.private"))
                .ok()?;
        Some(Self {
            file,
            line: zeroize::Zeroizing::new(Vec::new()),
            escape: 0,
            discarded: false,
            count: 0,
            active: true,
        })
    }
    pub(super) fn consume(&mut self, bytes: &[u8]) {
        if !self.active {
            return;
        }
        for &byte in bytes {
            if byte == b'\n' {
                if !self.discarded
                    && self.escape == 0
                    && let Some(event) = classify(&self.line)
                {
                    self.count += 1;
                    if self.count > 255 {
                        self.stop();
                        return;
                    }
                    if self.file.write_all(event).is_err() {
                        self.active = false;
                        return;
                    }
                }
                self.line.clear();
                self.escape = 0;
                self.discarded = false;
            } else if self.escape == 1 {
                if byte == b'[' {
                    self.escape = 2;
                } else {
                    self.discarded = true;
                    self.escape = 0;
                }
            } else if self.escape == 2 {
                if (0x40..=0x7e).contains(&byte) {
                    self.escape = 0;
                } else if !(0x20..=0x3f).contains(&byte) {
                    self.discarded = true;
                    self.escape = 0;
                }
            } else if byte == 0x1b {
                self.escape = 1;
            } else if byte != b'\r' && !self.discarded {
                if self.line.len() == 2048 {
                    self.stop();
                    return;
                }
                self.line.push(byte);
            }
        }
    }
    fn stop(&mut self) {
        let _ = self.file.write_all(b"limit\n");
        self.line.clear();
        self.active = false;
    }
}
fn classify(bytes: &[u8]) -> Option<&'static [u8]> {
    let line = std::str::from_utf8(bytes).ok()?;
    let (stamp, rest) = line.split_once(' ')?;
    if stamp.len() != 25 || stamp.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    let signatures = [
        (
            "DEBUG [agent:2267] Found session for: ",
            b"session-found\n".as_slice(),
        ),
        (
            "ERROR [agent:2264] Session not found in run_turn: ",
            b"session-missing\n".as_slice(),
        ),
        (
            "DEBUG [agent::thread:2546] Total messages in thread: ",
            b"resume-messages\n".as_slice(),
        ),
        (
            "DEBUG [agent::thread:2583] Total messages in thread: ",
            b"ordinary-send\n".as_slice(),
        ),
        (
            "DEBUG [agent::thread:2737] Starting agent turn execution",
            b"turn-started\n".as_slice(),
        ),
        (
            "DEBUG [agent::thread:2755] Turn execution completed",
            b"turn-completed\n".as_slice(),
        ),
        (
            "ERROR [agent::thread:2759] Turn execution failed: ",
            b"turn-failed\n".as_slice(),
        ),
        (
            "DEBUG [agent::thread:2747] Turn was cancelled, skipping cleanup",
            b"turn-cancelled\n".as_slice(),
        ),
    ];
    signatures
        .into_iter()
        .find_map(|(prefix, id)| rest.starts_with(prefix).then_some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sink(path: &std::path::Path) -> Events {
        Events {
            file: nan_harness_private_fs::open_private_new(path).unwrap(),
            line: zeroize::Zeroizing::new(Vec::new()),
            escape: 0,
            discarded: false,
            count: 0,
            active: true,
        }
    }
    #[test]
    fn split_ansi_crlf_discards_payload_and_emits_only_complete_source_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events");
        let mut events = sink(&path);
        events.consume(b"\x1b[");
        events
            .consume(b"32m2026-10-04T12:00:00+00:00 DEBUG [agent:2267] Found session for: PRIVATE");
        assert!(std::fs::read(&path).unwrap().is_empty());
        events.consume(b"\x1b[0m\r\nunknown PRIVATE\r\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"session-found\n");
        events.consume(b"notstamp DEBUG [agent:2267] Found session for: PRIVATE\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"session-found\n");
    }
    #[test]
    fn bounded_record_limit_is_closed_and_does_not_stop_drain() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events");
        let mut events = sink(&path);
        events.consume(&vec![b'x'; 524_289]);
        assert_eq!(std::fs::read(&path).unwrap(), b"limit\n");
        assert!(!events.active);
        events.consume(b"PRIVATE\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"limit\n");
    }
    #[test]
    fn missing_optin_has_no_extra_receipt() {
        let command = Command::new("/bin/sh");
        assert!(Events::prepare(&command).is_none());
    }
}
