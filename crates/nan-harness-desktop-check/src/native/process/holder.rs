//! A separate owned inspector retains verified handles across ordinary app shutdown.
use super::FailureCategory;
use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::JoinHandle,
    time::Instant,
};

pub(crate) struct Holder {
    child: Child,
    input: ChildStdin,
    output: Receiver<Option<String>>,
    reader: Option<JoinHandle<()>>,
    retained: usize,
    triggered: bool,
    native_anchor: u64,
    received_anchor: Instant,
}

fn progress(line: &str, completed: usize) -> Option<usize> {
    const STAGES: [&str; 7] = [
        "file-open",
        "file-hash",
        "file-identity",
        "process-open",
        "snapshot",
        "targets",
        "owner-recheck",
    ];
    let mut words = line.split_whitespace();
    if words.next()? != "progress"
        || words.next()? != *STAGES.get(completed)?
        || words.next().is_some()
    {
        return None;
    }
    Some(completed + 1)
}
fn progress_facts(completed: usize, outcome: &str) -> serde_json::Value {
    const STAGES: [&str; 8] = [
        "none",
        "file-open",
        "file-hash",
        "file-identity",
        "process-open",
        "snapshot",
        "targets",
        "owner-recheck",
    ];
    serde_json::json!({"schemaVersion":1,"mechanism":"windows-owned-cleanup-preflight-progress",
        "diagnosticsOnly":true,"completedStageCount":completed,"lastCompletedStage":STAGES[completed],"outcome":outcome})
}
fn record_progress(completed: usize, outcome: &str) {
    #[cfg(windows)]
    crate::process::windows_correlation::record_preflight_progress(&progress_facts(
        completed, outcome,
    ));
    #[cfg(not(windows))]
    let _ = progress_facts(completed, outcome);
}

pub(crate) fn ready(line: &str) -> Option<(usize, u64)> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.len() != 3 || words[0] != "ready" {
        return None;
    }
    let count = words[1].parse::<usize>().ok()?;
    let tick = words[2].parse::<u64>().ok()?;
    (count <= 64 && tick > 0).then_some((count, tick))
}
pub(crate) fn preflight(line: &str) -> Option<serde_json::Value> {
    let mut words = line.split_whitespace();
    if words.next()? != "unavailable" {
        return None;
    }
    let stage = words.next()?;
    if words.next().is_some()
        || !matches!(
            stage,
            "request"
                | "path"
                | "file-open"
                | "file-hash"
                | "file-identity"
                | "process-open"
                | "snapshot"
                | "inspector-parent"
                | "ancestry"
                | "target-open"
                | "target-identity"
                | "owner-recheck"
        )
    {
        return None;
    }
    Some(serde_json::json!({"schemaVersion":1,
        "mechanism":"windows-owned-cleanup-preflight","diagnosticsOnly":true,"stage":stage}))
}

fn cutoff(anchor: u64, received: Instant, deadline: Instant) -> Option<u64> {
    let elapsed = deadline.checked_duration_since(received)?.as_millis();
    // The native anchor predates its receipt. Margin also excludes coarse-clock
    // rounding; this cutoff shortens rather than renews the cleanup deadline.
    anchor
        .checked_add(u64::try_from(elapsed).ok()?)?
        .checked_sub(50)
}

impl Holder {
    // The bounded preflight handshake reports progress without granting target
    // authority; only the existing ready receipt admits the retained handles.
    fn receive_ready(&mut self, deadline: Instant) -> Result<(), FailureCategory> {
        let mut completed = 0;
        loop {
            let line = match self.line(deadline) {
                Ok(line) => line,
                Err(error) => {
                    record_progress(
                        completed,
                        if matches!(error, FailureCategory::Timeout) {
                            "deadline"
                        } else {
                            "transport"
                        },
                    );
                    return Err(error);
                }
            };
            if line.starts_with("progress ") {
                let Some(next) = progress(&line, completed) else {
                    record_progress(completed, "protocol");
                    return Err(FailureCategory::Output);
                };
                completed = next;
                continue;
            }
            if let Err(error) = self.accept_ready(&line) {
                record_progress(completed, "rejected");
                return Err(error);
            }
            record_progress(completed, "ready");
            break;
        }
        Ok(())
    }

    pub(crate) fn start(
        executable: &Path,
        launcher: u32,
        expected: &Path,
        digest: &str,
        deadline: Instant,
    ) -> Result<Self, FailureCategory> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(FailureCategory::InvalidInput);
        }
        if Instant::now() >= deadline {
            return Err(FailureCategory::Timeout);
        }
        let expected = expected
            .canonicalize()
            .map_err(|_| FailureCategory::InvalidInput)?;
        if expected
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case("Claude.exe"))
        {
            return Err(FailureCategory::InvalidInput);
        }
        let bytes = expected
            .to_str()
            .ok_or(FailureCategory::InvalidInput)?
            .as_bytes();
        if bytes.len() > 2048 || launcher == 0 {
            return Err(FailureCategory::InvalidInput);
        }
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(encoded, "{byte:02x}").map_err(|_| FailureCategory::InvalidInput)?;
        }
        let mut command = Command::new(executable);
        command
            .arg("--claude-owned-cleanup-holder")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|_| FailureCategory::Spawn)?;
        let Some(input) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FailureCategory::Pipe);
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FailureCategory::Pipe);
        };
        let (send, output) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            // Seven fixed progress lines, ready and result; no raw output export.
            for _ in 0..9 {
                let mut line = String::new();
                let result = reader.by_ref().take(129).read_line(&mut line);
                if !matches!(result, Ok(1..=128)) || !line.ends_with('\n') {
                    let _ = send.send(None);
                    return;
                }
                if send.send(Some(line)).is_err() {
                    return;
                }
            }
        });
        let mut holder = Self {
            child,
            input,
            output,
            reader: Some(reader),
            retained: 0,
            triggered: false,
            native_anchor: 0,
            received_anchor: Instant::now(),
        };
        writeln!(
            holder.input,
            "{} {launcher} {encoded} {digest}",
            std::process::id()
        )
        .map_err(|_| FailureCategory::Pipe)?;
        holder.receive_ready(deadline)?;
        Ok(holder)
    }

    fn accept_ready(&mut self, line: &str) -> Result<(), FailureCategory> {
        if let Some(value) = preflight(line) {
            #[cfg(windows)]
            crate::process::windows_correlation::record_preflight(&value);
            #[cfg(not(windows))]
            let _ = value;
            return Err(FailureCategory::Output);
        }
        (self.retained, self.native_anchor) = ready(line).ok_or(FailureCategory::Output)?;
        self.received_anchor = Instant::now();
        Ok(())
    }
    fn line(&self, deadline: Instant) -> Result<String, FailureCategory> {
        let duration = deadline
            .checked_duration_since(Instant::now())
            .ok_or(FailureCategory::Timeout)?;
        let line = self
            .output
            .recv_timeout(duration)
            .map_err(|_| FailureCategory::Timeout)?
            .ok_or(FailureCategory::Output)?;
        if Instant::now() >= deadline {
            return Err(FailureCategory::Timeout);
        }
        Ok(line)
    }
    pub(crate) fn cleanup(mut self, deadline: Instant) -> serde_json::Value {
        if let Some(cutoff) = cutoff(self.native_anchor, self.received_anchor, deadline)
            && deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                > 50
        {
            self.triggered = true;
            if writeln!(self.input, "cleanup {cutoff}").is_ok()
                && let Ok(line) = self.line(deadline)
                && let Some(value) = result(&line, self.retained)
            {
                return value;
            }
        }
        failure(
            if Instant::now() >= deadline {
                "deadline"
            } else {
                "uncertain"
            },
            Some(self.retained),
            self.triggered,
            true,
        )
    }
}
impl Drop for Holder {
    fn drop(&mut self) {
        // This is only the inspector child; retained app handles are never reopened.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(crate) fn failure(
    status: &str,
    retained: Option<usize>,
    triggered: bool,
    verified: bool,
) -> serde_json::Value {
    serde_json::json!({"schemaVersion":1,"mechanism":"windows-owned-descendant-cleanup","diagnosticsOnly":true,
        "status":status,"retainedCount":retained,"alreadyExitedCount":null,"targetedCount":null,"exitedCount":null,"rejectedCount":null,
        "triggerAttempted":triggered,"expectedExecutableVerified":verified,"historicalOwnershipVerified":verified})
}
pub(crate) fn result(line: &str, retained: usize) -> Option<serde_json::Value> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.len() != 6 || words[0] != "result" {
        return None;
    }
    let counts = words[1..]
        .iter()
        .map(|value| value.parse::<usize>().ok())
        .collect::<Option<Vec<_>>>()?;
    if counts[0] != retained
        || counts.iter().any(|n| *n > 64)
        || counts[1] + counts[2] + counts[4] != retained
        || counts[3] > counts[2]
    {
        return None;
    }
    Some(
        serde_json::json!({"schemaVersion":1,"mechanism":"windows-owned-descendant-cleanup","diagnosticsOnly":true,
        "status":if counts[4]==0&&counts[3]==counts[2]{"completed"}else{"partial"},"retainedCount":retained,
        "alreadyExitedCount":counts[1],"targetedCount":counts[2],"exitedCount":counts[3],"rejectedCount":counts[4],
        "triggerAttempted":true,"expectedExecutableVerified":true,"historicalOwnershipVerified":true}),
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn progress_is_monotonic_bounded_and_closes_unknown_text() {
        let stages = [
            "file-open",
            "file-hash",
            "file-identity",
            "process-open",
            "snapshot",
            "targets",
            "owner-recheck",
        ];
        for (index, stage) in stages.iter().enumerate() {
            assert_eq!(
                progress(&format!("progress {stage}\n"), index),
                Some(index + 1)
            );
            let facts = progress_facts(index + 1, "deadline");
            assert_eq!(facts["completedStageCount"], index + 1);
            assert_eq!(facts["lastCompletedStage"], *stage);
            assert_eq!(facts.as_object().unwrap().len(), 6);
        }
        for line in [
            "progress",
            "progress secret",
            "progress file-open extra",
            "progress snapshot",
        ] {
            assert!(progress(line, 0).is_none());
        }
        assert!(progress("progress file-open", 1).is_none());
        assert!(progress("progress owner-recheck", 7).is_none());
    }

    use super::*;
    #[test]
    fn preflight_classifies_only_closed_complete_native_rejections() {
        for stage in [
            "request",
            "path",
            "file-open",
            "file-hash",
            "file-identity",
            "process-open",
            "snapshot",
            "inspector-parent",
            "ancestry",
            "target-open",
            "target-identity",
            "owner-recheck",
        ] {
            let value = preflight(&format!("unavailable {stage}\n")).unwrap();
            assert_eq!(value["stage"], stage);
            assert_eq!(value.as_object().unwrap().len(), 4);
        }
        for line in [
            "unavailable",
            "unavailable secret",
            "unavailable path private",
            "ready path",
            "unavailable C:\\private",
        ] {
            assert!(preflight(line).is_none());
        }
    }

    #[test]
    fn cutoff_uses_held_clock_and_shortens_original_deadline() {
        let received = Instant::now();
        assert_eq!(
            cutoff(1000, received, received + std::time::Duration::from_secs(5)),
            Some(5950)
        );
        assert_eq!(
            cutoff(
                1000,
                received,
                received
                    .checked_sub(std::time::Duration::from_millis(1))
                    .unwrap()
            ),
            None
        );
        assert_eq!(
            cutoff(
                u64::MAX,
                received,
                received + std::time::Duration::from_secs(5)
            ),
            None
        );
    }
    #[test]
    fn uncertain_cleanup_keeps_only_closed_known_preflight() {
        let value = failure("deadline", Some(5), true, true);
        assert_eq!(value["targetedCount"], serde_json::Value::Null);
        assert_eq!(value["triggerAttempted"], true);
        let unavailable = failure("unavailable", None, false, false);
        assert_eq!(unavailable["historicalOwnershipVerified"], false);
    }
    #[cfg(not(windows))]
    #[test]
    fn bounded_holder_transport_has_one_trigger_and_no_late_success() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("helper");
        let expected = directory.path().join("Claude.exe");
        std::fs::write(&expected, []).unwrap();
        std::fs::write(&helper,"#!/bin/sh\nread request\nprintf 'ready 0 100\\n'\nread trigger\nprintf 'result 0 0 0 0 0\\n'\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let holder = Holder::start(&helper, 1, &expected, &"0".repeat(64), deadline).unwrap();
        assert_eq!(holder.cleanup(deadline)["status"], "completed");
        let holder = Holder::start(&helper, 1, &expected, &"0".repeat(64), deadline).unwrap();
        let value = holder.cleanup(Instant::now());
        assert_eq!(value["status"], "deadline");
        assert_eq!(value["triggerAttempted"], false);
        assert!(Holder::start(&helper, 1, &expected, &"0".repeat(64), Instant::now()).is_err());
    }
    #[cfg(not(windows))]
    #[test]
    fn progress_handshake_rejects_reordering_and_allows_one_cleanup_trigger() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("helper");
        let expected = directory.path().join("Claude.exe");
        std::fs::write(&expected, []).unwrap();
        let stages = [
            "file-open",
            "file-hash",
            "file-identity",
            "process-open",
            "snapshot",
            "targets",
            "owner-recheck",
        ];
        let valid = stages.map(|stage| format!("progress {stage}\n")).join("");
        for (frames, accepted) in [
            (valid, true),
            ("progress snapshot\n".into(), false),
            ("progress file-open\nprogress file-open\n".into(), false),
            ("progress private-message\n".into(), false),
        ] {
            let script = format!(
                "#!/bin/sh\nread request\nprintf '{frames}ready 0 100\\n'\nread trigger\nprintf 'result 0 0 0 0 0\\n'\n"
            );
            std::fs::write(&helper, script).unwrap();
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
            let deadline = Instant::now() + std::time::Duration::from_secs(2);
            let holder = Holder::start(&helper, 1, &expected, &"0".repeat(64), deadline);
            if accepted {
                assert_eq!(holder.unwrap().cleanup(deadline)["status"], "completed");
            } else {
                assert!(holder.is_err());
            }
        }
    }

    #[test]
    fn protocol_requires_complete_partition_and_never_exports_identity() {
        assert_eq!(ready("ready 5 100\n"), Some((5, 100)));
        assert_eq!(ready("ready 65 100\n"), None);
        assert_eq!(
            result("result 5 1 4 4 0", 5).unwrap()["status"],
            "completed"
        );
        assert_eq!(result("result 5 1 3 2 1", 5).unwrap()["status"], "partial");
        for line in [
            "result 5 1 4 5 0",
            "result 5 1 3 3 0",
            "result 6 0 6 6 0",
            "result 5 1 4 4 0 PRIVATE",
        ] {
            assert!(result(line, 5).is_none());
        }
        assert!(
            !result("result 5 1 4 4 0", 5)
                .unwrap()
                .to_string()
                .contains("pid")
        );
    }
}
