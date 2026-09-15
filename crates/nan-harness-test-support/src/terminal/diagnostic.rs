//! Owned process diagnostics. Payloads stay private; callers publish only typed observations.
use super::*;
use std::io::{Read as _, Seek as _};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod survivors;
pub use survivors::{ScanState, SurvivorScan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Pipe,
    PrivateFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessEvent {
    Exited,
    TimedOut,
    Cancelled,
    Failed,
}

/// Outcome of one stream reader while the case budget ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderOutcome {
    /// The reader observed end of file before the capture deadline.
    Eof,
    /// The reader was still waiting, which means some process still held the write end.
    Open,
    /// The reader failed on the pipe itself instead of observing end of file.
    Error,
}

/// Captured bytes must never be serialized into public reports.
#[derive(Debug)]
pub struct DiagnosticOutput {
    pub event: ProcessEvent,
    pub capture_mode: CaptureMode,
    pub cleanup_stage: Option<CleanupStage>,
    pub status: Option<ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_eof: bool,
    pub stderr_eof: bool,
    pub stdout_eof_after: Option<Duration>,
    pub stderr_eof_after: Option<Duration>,
    /// Reader state at the capture deadline, independent for each stream.
    pub stdout_reader: ReaderOutcome,
    pub stderr_reader: ReaderOutcome,
    /// End of file observed only after the owned tree was terminated; `Some` proves the
    /// surviving writer was owned, and `None` leaves the writer unidentified.
    pub stdout_eof_after_cleanup: Option<Duration>,
    pub stderr_eof_after_cleanup: Option<Duration>,
    /// Live descendants of the launched root while the capture still had not reached end of file.
    pub survivors_at_failure: SurvivorScan,
    /// Live descendants of the launched root after the case finished terminating its owned tree.
    pub survivors_residual: SurvivorScan,
    /// None means no termination was attempted; Some(false) means cleanup is unconfirmed.
    pub cleanup: Option<bool>,
    pub os_error_code: Option<u32>,
    pub duration: Duration,
}

impl DiagnosticOutput {
    fn empty(capture_mode: CaptureMode) -> Self {
        Self {
            event: ProcessEvent::Failed,
            capture_mode,
            cleanup_stage: None,
            status: None,
            stdout: String::new(),
            stderr: String::new(),
            stdout_eof: false,
            stderr_eof: false,
            stdout_eof_after: None,
            stderr_eof_after: None,
            stdout_reader: ReaderOutcome::Open,
            stderr_reader: ReaderOutcome::Open,
            stdout_eof_after_cleanup: None,
            stderr_eof_after_cleanup: None,
            survivors_at_failure: SurvivorScan::not_needed(),
            survivors_residual: SurvivorScan::not_needed(),
            cleanup: None,
            os_error_code: None,
            duration: Duration::ZERO,
        }
    }
}

impl TerminalCommand {
    /// Runs with one absolute budget, including stream collection and owned-process cleanup.
    /// Cancellation is an independent timer, not a shortened timeout. File captures are protected
    /// before spawning and are never interpreted as pipe EOF observations.
    pub async fn diagnose(
        self,
        mode: CaptureMode,
        cancel_after: Option<Duration>,
    ) -> DiagnosticOutput {
        let started = Instant::now();
        let mut result = DiagnosticOutput::empty(mode);
        let deadline = tokio::time::Instant::now() + self.timeout;
        // Reserve cleanup time inside the caller's budget instead of adding it after expiry.
        let execution = self
            .timeout
            .saturating_sub(self.timeout.min(Duration::from_secs(3)) / 2);
        // The capture window and the cleanup slice partition the same budget, so observing end of
        // file after cleanup never extends the case beyond the caller's timeout.
        let capture_window = execution / 4;
        let cleanup_slice = execution.saturating_sub(capture_window);
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .current_dir(&self.current_directory)
            .kill_on_drop(true);
        if self.clear_environment {
            command.env_clear();
        }
        command.envs(&self.environment).stdin(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        let files = if mode == CaptureMode::PrivateFile {
            match private_captures(&self.current_directory, &mut command) {
                Ok(files) => Some(files),
                Err(error) => {
                    result.os_error_code = os_error_code(&error);
                    return result;
                }
            }
        } else {
            None
        };
        if mode == CaptureMode::Pipe {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = match spawn_owned(command) {
            Ok(child) => child,
            Err(error) => {
                result.os_error_code = os_error_code(&error);
                return result;
            }
        };
        let pid = child_id(&child);
        let out_state = Arc::new(Mutex::new(Captured::default()));
        let err_state = Arc::new(Mutex::new(Captured::default()));
        let captures = if mode == CaptureMode::Pipe {
            match (child_stdout(&mut child), child_stderr(&mut child)) {
                (Some(stdout), Some(stderr)) => Some((
                    tokio::spawn(capture(stdout, Arc::clone(&out_state), started)),
                    tokio::spawn(capture(stderr, Arc::clone(&err_state), started)),
                )),
                _ => None,
            }
        } else {
            None
        };
        let cancel =
            tokio::time::sleep(cancel_after.unwrap_or(self.timeout + Duration::from_secs(1)));
        tokio::pin!(cancel);
        tokio::select! {
            status = child_wait(&mut child) => match status {
                Ok(status) => { result.event = ProcessEvent::Exited; result.status = Some(status); },
                Err(error) => result.os_error_code = os_error_code(&error),
            },
            () = tokio::time::sleep(execution) => result.event = ProcessEvent::TimedOut,
            () = &mut cancel => result.event = ProcessEvent::Cancelled,
        }
        {
            let mut observation = Observation {
                child: &mut child,
                pid,
                deadline,
                capture_window,
                cleanup_slice,
                result: &mut result,
            };
            let cleaned = observation.result.event != ProcessEvent::Exited;
            if cleaned {
                observation.clean().await;
            }
            if let Some((stdout, stderr)) = captures {
                observation
                    .collect_pipes(stdout, stderr, &out_state, &err_state, cleaned)
                    .await;
            }
            if let Some((stdout, stderr)) = files {
                observation.collect_files(stdout, stderr).await;
            }
            observation.observe_residual().await;
        }
        result.duration = started.elapsed();
        result
    }
}

/// Per-case ownership evidence, budget, and timeline for one observed process.
struct Observation<'a> {
    child: &'a mut OwnedChild,
    pid: Option<u32>,
    deadline: tokio::time::Instant,
    capture_window: Duration,
    cleanup_slice: Duration,
    result: &'a mut DiagnosticOutput,
}

impl Observation<'_> {
    async fn clean(&mut self) {
        clean(self.child, self.pid, self.deadline, self.result).await;
    }

    /// Collects both streams and attributes every writer that never closed.
    ///
    /// Both streams progress concurrently, so a completed stream survives its peer hanging. The
    /// readers stay alive across cleanup: aborting them earlier made an owned surviving writer
    /// indistinguishable from an unowned one, because the only remaining evidence is the pipe.
    async fn collect_pipes(
        &mut self,
        mut stdout: tokio::task::JoinHandle<std::io::Result<()>>,
        mut stderr: tokio::task::JoinHandle<std::io::Result<()>>,
        out_state: &Arc<Mutex<Captured>>,
        err_state: &Arc<Mutex<Captured>>,
        cleaned: bool,
    ) {
        let capture_deadline = self
            .deadline
            .min(tokio::time::Instant::now() + self.capture_window);
        let (out, err) = tokio::join!(
            tokio::time::timeout_at(capture_deadline, &mut stdout),
            tokio::time::timeout_at(capture_deadline, &mut stderr)
        );
        (self.result.stdout, self.result.stdout_eof_after) = capture_snapshot(out_state);
        (self.result.stderr, self.result.stderr_eof_after) = capture_snapshot(err_state);
        self.result.stdout_reader = reader_outcome(&out);
        self.result.stderr_reader = reader_outcome(&err);
        self.result.stdout_eof = self.result.stdout_reader == ReaderOutcome::Eof;
        self.result.stderr_eof = self.result.stderr_reader == ReaderOutcome::Eof;
        if self.result.stdout_eof && self.result.stderr_eof {
            stdout.abort();
            stderr.abort();
            return;
        }
        // Name the surviving writers while the pipe is still open, before any cleanup could
        // terminate them.
        self.result.survivors_at_failure = self.survivors_now().await;
        if !cleaned {
            self.clean().await;
        }
        let observe_deadline = self
            .deadline
            .min(tokio::time::Instant::now() + self.cleanup_slice.min(Duration::from_secs(3)));
        let (post_out, post_err) = tokio::join!(
            tokio::time::timeout_at(observe_deadline, &mut stdout),
            tokio::time::timeout_at(observe_deadline, &mut stderr)
        );
        if matches!(post_out, Ok(Ok(Ok(())))) {
            self.result.stdout_eof_after_cleanup = capture_snapshot(out_state).1;
        }
        if matches!(post_err, Ok(Ok(Ok(())))) {
            self.result.stderr_eof_after_cleanup = capture_snapshot(err_state).1;
        }
        if self.result.event == ProcessEvent::Exited {
            self.result.event = ProcessEvent::Failed;
        }
        stdout.abort();
        stderr.abort();
    }

    async fn collect_files(&mut self, mut stdout: std::fs::File, mut stderr: std::fs::File) {
        self.clean().await;
        match (read_file(&mut stdout), read_file(&mut stderr)) {
            (Ok(stdout), Ok(stderr)) => {
                self.result.stdout = stdout;
                self.result.stderr = stderr;
            }
            _ => self.result.event = ProcessEvent::Failed,
        }
    }

    /// Reads the live descendants of the case's root without exceeding the case budget.
    async fn survivors_now(&self) -> SurvivorScan {
        let Some(pid) = self.pid else {
            return SurvivorScan::unavailable();
        };
        let remaining = self
            .deadline
            .saturating_duration_since(tokio::time::Instant::now())
            .min(Duration::from_secs(2));
        if remaining.is_zero() {
            return SurvivorScan::unavailable();
        }
        tokio::time::timeout(remaining, survivors::scan(pid))
            .await
            .unwrap_or_else(|_| SurvivorScan::unavailable())
    }

    /// Proves whether any descendant outlived the owned tree, which the file control cannot show
    /// on its own. The observation stays inside the case budget.
    async fn observe_residual(&mut self) {
        let remaining = self
            .deadline
            .saturating_duration_since(tokio::time::Instant::now());
        let Some(pid) = self.pid else {
            return;
        };
        if remaining.is_zero() {
            return;
        }
        self.result.survivors_residual =
            tokio::time::timeout(remaining, survivors::scan_after_cleanup(pid))
                .await
                .unwrap_or_else(|_| SurvivorScan::unavailable());
    }
}

async fn clean(
    child: &mut OwnedChild,
    pid: Option<u32>,
    deadline: tokio::time::Instant,
    result: &mut DiagnosticOutput,
) {
    result.cleanup = Some(false);
    result.cleanup_stage = Some(CleanupStage::WaitTimeout);
    if let Ok(cleanup) =
        tokio::time::timeout_at(deadline, terminate_owned_process(child, pid)).await
    {
        result.cleanup_stage = Some(cleanup.stage);
        result.cleanup =
            Some(cleanup.stage == CleanupStage::CaptureTimeout && cleanup.os_error_code.is_none());
        result.os_error_code = cleanup.os_error_code.or(result.os_error_code);
    }
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    eof_after: Option<Duration>,
}

fn capture_snapshot(state: &Mutex<Captured>) -> (String, Option<Duration>) {
    let state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (
        String::from_utf8_lossy(&state.bytes).into_owned(),
        state.eof_after,
    )
}

fn reader_outcome<T>(
    waited: &Result<
        Result<Result<T, std::io::Error>, tokio::task::JoinError>,
        tokio::time::error::Elapsed,
    >,
) -> ReaderOutcome {
    match waited {
        Ok(Ok(Ok(_))) => ReaderOutcome::Eof,
        Ok(Ok(Err(_)) | Err(_)) => ReaderOutcome::Error,
        Err(_) => ReaderOutcome::Open,
    }
}

async fn capture(
    mut stream: impl AsyncRead + Unpin,
    state: Arc<Mutex<Captured>>,
    started: Instant,
) -> std::io::Result<()> {
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream.read(&mut buffer).await?;
        let mut retained = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if count == 0 {
            retained.eof_after = Some(started.elapsed());
            return Ok(());
        }
        let keep = count.min(MAX_CAPTURE_BYTES.saturating_sub(retained.bytes.len()));
        retained.bytes.extend_from_slice(&buffer[..keep]);
    }
}

fn private_captures(
    directory: &Path,
    command: &mut Command,
) -> std::io::Result<(std::fs::File, std::fs::File)> {
    let stdout =
        nan_harness_private_fs::open_private_read_write(&directory.join("stdout.private"))?;
    let stderr =
        nan_harness_private_fs::open_private_read_write(&directory.join("stderr.private"))?;
    command
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    Ok((stdout, stderr))
}

fn read_file(file: &mut std::fs::File) -> std::io::Result<String> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(MAX_CAPTURE_BYTES as u64)
        .read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn exited_parent_with_inherited_pipe_keeps_partial_evidence() {
        let root = tempfile::tempdir().unwrap();
        let output = TerminalCommand::new("/bin/sh", root.path())
            .args(["-c", "printf PARTIAL_MARKER; sleep 120 &"])
            .timeout(Duration::from_secs(2))
            .diagnose(CaptureMode::Pipe, None)
            .await;
        assert_eq!(output.event, ProcessEvent::Failed);
        assert_eq!(output.status.and_then(|status| status.code()), Some(0));
        assert!(output.stdout.contains("PARTIAL_MARKER"));
        assert!(!output.stdout_eof);
        assert_eq!(output.stdout_eof_after, None);
        assert_eq!(output.stdout_reader, ReaderOutcome::Open);
        assert_eq!(output.cleanup, Some(true));
        // Killing the owned tree releases the pipe, and the observation must prove it.
        assert!(
            output.stdout_eof_after_cleanup.is_some(),
            "owned descendants released stdout after cleanup: {output:?}"
        );
        assert!(
            output.stderr_eof_after_cleanup.is_some(),
            "owned descendants released stderr after cleanup: {output:?}"
        );
        // The surviving writer is named while the pipe is still open and gone afterwards.
        assert_eq!(output.survivors_at_failure.state, ScanState::Available);
        assert!(
            output.survivors_at_failure.count >= 1
                && output
                    .survivors_at_failure
                    .names
                    .iter()
                    .any(|name| name.starts_with("sleep")),
            "the surviving writer should be named: {:?}",
            output.survivors_at_failure
        );
        assert_eq!(output.survivors_residual.state, ScanState::Available);
        assert_eq!(
            output.survivors_residual.count, 0,
            "the owned tree must not outlive cleanup: {:?}",
            output.survivors_residual
        );
    }

    fn fixture(path: &Path, long: bool) -> TerminalCommand {
        if cfg!(windows) {
            TerminalCommand::new("cmd", path).args([
                "/d",
                "/c",
                if long {
                    "ping 127.0.0.1 -n 120 > nul"
                } else {
                    "echo DIAGNOSTIC_OK"
                },
            ])
        } else {
            TerminalCommand::new("/bin/sh", path).args([
                "-c",
                if long {
                    "sleep 120"
                } else {
                    "printf DIAGNOSTIC_OK"
                },
            ])
        }
        .timeout(Duration::from_secs(2))
    }

    #[tokio::test]
    async fn real_pipe_and_private_file_captures_are_distinct() {
        for mode in [CaptureMode::Pipe, CaptureMode::PrivateFile] {
            let root = tempfile::tempdir().unwrap();
            let output = fixture(root.path(), false).diagnose(mode, None).await;
            assert_eq!(output.event, ProcessEvent::Exited);
            assert!(output.stdout.contains("DIAGNOSTIC_OK"));
            assert_eq!(output.stdout_eof, mode == CaptureMode::Pipe);
            assert_eq!(
                root.path().join("stdout.private").exists(),
                mode == CaptureMode::PrivateFile
            );
            assert_ne!(output.cleanup, Some(false));
            #[cfg(unix)]
            if mode == CaptureMode::PrivateFile {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    std::fs::metadata(root.path().join("stdout.private"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
    }

    #[tokio::test]
    async fn cancellation_and_timeout_are_independent_owned_events() {
        for (cancel, event) in [
            (Some(Duration::from_millis(50)), ProcessEvent::Cancelled),
            (None, ProcessEvent::TimedOut),
        ] {
            let root = tempfile::tempdir().unwrap();
            let output = fixture(root.path(), true)
                .diagnose(CaptureMode::Pipe, cancel)
                .await;
            assert_eq!(output.event, event);
            assert_eq!(output.cleanup, Some(true));
            assert!(output.stdout_eof && output.stderr_eof);
            assert_eq!(output.stdout_reader, ReaderOutcome::Eof);
            assert_eq!(output.stderr_reader, ReaderOutcome::Eof);
            assert!(output.duration < Duration::from_secs(2));
        }
    }

    #[tokio::test]
    async fn missing_executable_does_not_claim_exit_or_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let output = TerminalCommand::new(root.path().join("missing"), root.path())
            .diagnose(CaptureMode::Pipe, None)
            .await;
        assert_eq!(output.event, ProcessEvent::Failed);
        assert!(output.status.is_none());
        assert_eq!(output.cleanup, None);
        assert!(!output.stdout_eof);
    }

    #[tokio::test]
    async fn retention_cap_is_not_eof_and_does_not_block_writer() {
        use tokio::io::AsyncWriteExt as _;
        let (mut writer, reader) = tokio::io::duplex(4096);
        let state = Arc::new(Mutex::new(Captured::default()));
        let task = tokio::spawn(capture(reader, Arc::clone(&state), Instant::now()));
        writer
            .write_all(&vec![b'x'; MAX_CAPTURE_BYTES * 2])
            .await
            .unwrap();
        assert!(!task.is_finished());
        drop(writer);
        task.await.unwrap().unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.bytes.len(), MAX_CAPTURE_BYTES);
        assert!(state.eof_after.is_some());
    }
}
