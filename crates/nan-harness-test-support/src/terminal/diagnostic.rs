//! Owned process diagnostics. Payloads stay private; callers publish only typed observations.
use super::*;
use std::io::{Read as _, Seek as _};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            None
        };
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
        if result.event != ProcessEvent::Exited {
            clean(&mut child, pid, deadline, &mut result).await;
        }
        if let Some((mut stdout, mut stderr)) = captures {
            // Both streams progress concurrently; preserve a completed stream if its peer hangs.
            let capture_deadline = deadline.min(tokio::time::Instant::now() + execution / 4);
            let (out, err) = tokio::join!(
                tokio::time::timeout_at(capture_deadline, &mut stdout),
                tokio::time::timeout_at(capture_deadline, &mut stderr)
            );
            (result.stdout, result.stdout_eof_after) = capture_snapshot(&out_state);
            (result.stderr, result.stderr_eof_after) = capture_snapshot(&err_state);
            result.stdout_eof = matches!(out, Ok(Ok(Ok(())))) && result.stdout_eof_after.is_some();
            result.stderr_eof = matches!(err, Ok(Ok(Ok(())))) && result.stderr_eof_after.is_some();
            stdout.abort();
            stderr.abort();
            if !result.stdout_eof || !result.stderr_eof {
                clean(&mut child, pid, deadline, &mut result).await;
                if result.event == ProcessEvent::Exited {
                    result.event = ProcessEvent::Failed;
                }
            }
        }
        if let Some((mut stdout, mut stderr)) = files {
            clean(&mut child, pid, deadline, &mut result).await;
            match (read_file(&mut stdout), read_file(&mut stderr)) {
                (Ok(stdout), Ok(stderr)) => {
                    result.stdout = stdout;
                    result.stderr = stderr;
                }
                _ => result.event = ProcessEvent::Failed,
            }
        }
        result.duration = started.elapsed();
        result
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
        assert_eq!(output.cleanup, Some(true));
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
