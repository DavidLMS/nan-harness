//! Bounded diagnostic framing for the existing single macOS Chat helper action.
use super::{FailureCategory, MAX_OUTPUT, write_input};
use crate::native::mac_chat_turn::{NativeProgress, TransportObservation};
use std::{
    io::{BufRead as _, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

fn read_result(
    output: impl std::io::Read,
    latest: &Mutex<Option<NativeProgress>>,
) -> Result<Zeroizing<String>, FailureCategory> {
    // Progress has at most sixteen fixed frames. Keep the original receipt cap
    // separately; no unbounded private output or diagnostic text is retained.
    let mut input = BufReader::new(output.take(MAX_OUTPUT + 16 * 128 + 1));
    let mut receipt = Zeroizing::new(Vec::new());
    let mut previous = None;
    loop {
        let mut line = Zeroizing::new(Vec::new());
        let count = input
            .read_until(b'\n', &mut line)
            .map_err(|_| FailureCategory::Output)?;
        if count == 0 {
            break;
        }
        if receipt.is_empty() && line.starts_with(b"progress ") {
            if line.len() > 128 {
                return Err(FailureCategory::Output);
            }
            let stage = NativeProgress::parse(&line).ok_or(FailureCategory::Output)?;
            if previous.is_some_and(|prior| stage as u8 <= prior) {
                return Err(FailureCategory::Output);
            }
            previous = Some(stage as u8);
            *latest.lock().map_err(|_| FailureCategory::Output)? = Some(stage);
        } else {
            receipt.extend_from_slice(&line);
            if receipt.len() as u64 > MAX_OUTPUT {
                return Err(FailureCategory::Output);
            }
        }
    }
    String::from_utf8(receipt.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| FailureCategory::Output)
}

pub(super) fn run(
    executable: &Path,
    input: &[u8],
    deadline: Instant,
    observation: &mut TransportObservation,
) -> Result<Zeroizing<String>, FailureCategory> {
    if Instant::now() >= deadline {
        return Err(FailureCategory::Timeout);
    }
    let mut command = Command::new(executable);
    command
        .env_clear()
        .arg("--claude-chat-turn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in ["DISPLAY", "XAUTHORITY", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn().map_err(|_| FailureCategory::Spawn)?;
    observation.started = true;
    let stdin = child.stdin.take().ok_or(FailureCategory::Pipe)?;
    let stdout = child.stdout.take().ok_or(FailureCategory::Pipe)?;
    let latest = Arc::new(Mutex::new(None));
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || write_input(stdin, None, input));
        let reader_progress = Arc::clone(&latest);
        let reader = scope.spawn(move || read_result(stdout, &reader_progress));
        let status = loop {
            if Instant::now() >= deadline {
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) => std::thread::sleep(
                    Duration::from_millis(20)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join();
        let output = reader.join();
        observation.progress = latest.lock().ok().and_then(|value| *value);
        let written = written.map_err(|_| FailureCategory::Pipe)?;
        let output = output.map_err(|_| FailureCategory::Pipe)?;
        // Even a complete Sent receipt observed during teardown cannot change
        // the original deadline/nonzero verdict or increment conversation state.
        if status.is_none() || Instant::now() >= deadline {
            return Err(FailureCategory::Timeout);
        }
        if !status.is_some_and(|status| status.success()) {
            return Err(FailureCategory::NonzeroExit);
        }
        written?;
        output
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::native::mac_chat_turn::ChatTurnReceipt;
    use std::os::unix::fs::PermissionsExt as _;

    fn scripted(
        suffix: &str,
        budget: Duration,
        observation: &mut TransportObservation,
    ) -> Result<Zeroizing<String>, FailureCategory> {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("helper");
        let count = directory.path().join("calls");
        std::fs::write(&executable, format!(
            "#!/usr/bin/env python3\nimport sys,time\nif '--version' in sys.argv: raise SystemExit(0)\nwith open({:?},'a') as f: f.write('attempt\\n')\nwire=sys.stdin.buffer.read()\nassert wire==b'private synthetic request\\n'\n{suffix}\n", count.to_str().unwrap()
        )).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        nan_harness_test_support::executable_fixture::wait_until_ready(&executable).unwrap();
        let result = run(
            &executable,
            b"private synthetic request",
            Instant::now() + budget,
            observation,
        );
        assert_eq!(std::fs::read_to_string(count).unwrap(), "attempt\n");
        result
    }

    #[test]
    fn progress_is_closed_ordered_bounded_and_separate_from_the_receipt() {
        let latest = Mutex::new(None);
        let raw = b"progress window\nprogress tree\nprogress finish\nturn sent\ncomposer 1 0 1\n";
        let output = read_result(&raw[..], &latest).unwrap();
        assert_eq!(*latest.lock().unwrap(), Some(NativeProgress::Finish));
        assert!(ChatTurnReceipt::parse(&output, "input-failure-owned").is_some());
        for invalid in [
            b"progress tree\nprogress tree\n".to_vec(),
            b"progress tree\nprogress window\n".to_vec(),
            b"progress tree\nprogress PRIVATE\n".to_vec(),
            b"progress tree\nprogress finish PRIVATE\n".to_vec(),
            b"progress tree\nprogress finish".to_vec(),
            [b"progress tree\nprogress ".as_slice(), &vec![b'x'; 1024]].concat(),
            [
                b"progress tree\nturn ".as_slice(),
                &vec![b'x'; usize::try_from(MAX_OUTPUT).unwrap()],
            ]
            .concat(),
        ] {
            let latest = Mutex::new(None);
            assert_eq!(
                read_result(invalid.as_slice(), &latest),
                Err(FailureCategory::Output)
            );
            assert_eq!(*latest.lock().unwrap(), Some(NativeProgress::Tree));
        }
        assert!(NativeProgress::parse(b"progress {\"tree\":null}\n").is_none());
        assert!(NativeProgress::parse(b"progress tree\r\n").is_none());
    }

    #[test]
    fn progress_survives_timeout_without_promoting_even_a_complete_sent_receipt() {
        for script in [
            "print('progress window\\nprogress tree',flush=True);time.sleep(2)",
            "print('progress window\\nprogress finish\\nturn sent\\ncomposer 1 0 1',flush=True);time.sleep(2)",
        ] {
            let mut observation = TransportObservation::default();
            let result = scripted(script, Duration::from_millis(500), &mut observation);
            assert_eq!(result, Err(FailureCategory::Timeout));
            assert!(observation.started);
            assert!(matches!(
                observation.progress,
                Some(NativeProgress::Tree | NativeProgress::Finish)
            ));
        }
    }

    #[test]
    fn nonzero_and_malformed_output_keep_the_closed_last_site_without_success() {
        for (script, expected) in [
            (
                "print('progress tree\\nprogress finish\\nturn sent\\ncomposer 1 0 1',flush=True);raise SystemExit(3)",
                FailureCategory::NonzeroExit,
            ),
            (
                "print('progress tree\\nprogress PRIVATE',flush=True)",
                FailureCategory::Output,
            ),
        ] {
            let mut observation = TransportObservation::default();
            assert_eq!(
                scripted(script, Duration::from_secs(2), &mut observation),
                Err(expected)
            );
            assert!(observation.started);
            assert!(observation.progress.is_some());
        }
    }

    #[test]
    fn expiry_and_spawn_failure_never_claim_a_started_helper() {
        let mut observation = TransportObservation::default();
        let missing = Path::new("/missing-private-helper");
        assert_eq!(
            run(missing, b"", Instant::now(), &mut observation),
            Err(FailureCategory::Timeout)
        );
        assert!(!observation.started);
        assert!(observation.progress.is_none());
        assert_eq!(
            run(
                missing,
                b"",
                Instant::now() + Duration::from_secs(1),
                &mut observation
            ),
            Err(FailureCategory::Spawn)
        );
        assert!(!observation.started);
        assert!(observation.progress.is_none());
    }
}
