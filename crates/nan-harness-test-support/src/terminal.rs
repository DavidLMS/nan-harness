use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt as _};
#[cfg(not(windows))]
use tokio::process::Child;
use tokio::process::Command;

#[cfg(windows)]
type OwnedChild = Box<dyn process_wrap::tokio::ChildWrapper>;
#[cfg(not(windows))]
type OwnedChild = Child;

const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct TerminalResponse {
    prompt: String,
    response: String,
}

#[derive(Debug, Clone)]
pub struct TerminalCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    current_directory: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    terminal_response: Option<TerminalResponse>,
    timeout: Duration,
    clear_environment: bool,
}

impl TerminalCommand {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, current_directory: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            current_directory: current_directory.into(),
            environment: BTreeMap::new(),
            terminal_response: None,
            timeout: Duration::from_mins(1),
            clear_environment: false,
        }
    }

    #[must_use]
    pub fn args<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.arguments.extend(arguments.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub const fn clear_environment(mut self) -> Self {
        self.clear_environment = true;
        self
    }

    #[must_use]
    pub fn respond_when(mut self, prompt: impl Into<String>, response: impl Into<String>) -> Self {
        self.terminal_response = Some(TerminalResponse {
            prompt: prompt.into(),
            response: response.into(),
        });
        self
    }

    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs the command with bounded captured output and a hard timeout.
    ///
    /// # Errors
    ///
    /// Returns [`TerminalError`] when the process cannot start, exceeds its timeout, or cannot be
    /// reaped.
    pub async fn run(self) -> Result<TerminalOutput, TerminalError> {
        let mut command = if let Some(response) = &self.terminal_response {
            let mut command = Command::new("/usr/bin/expect");
            command
                .arg("-c")
                .arg(expect_script(&self.program, &self.arguments, response));
            command
        } else {
            let mut command = Command::new(&self.program);
            command.args(&self.arguments);
            command
        };
        if self.clear_environment {
            command.env_clear();
        }
        command
            .current_dir(&self.current_directory)
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        let mut child = spawn_owned(command).map_err(|source| TerminalError::Execute {
            program: self.program.clone(),
            source,
        })?;
        let pid = child_id(&child);
        let stdout = child_stdout(&mut child).ok_or_else(|| TerminalError::MissingOutput {
            stream: "stdout",
            program: self.program.clone(),
        })?;
        let stderr = child_stderr(&mut child).ok_or_else(|| TerminalError::MissingOutput {
            stream: "stderr",
            program: self.program.clone(),
        })?;
        let stdout_task = tokio::spawn(capture_output(stdout));
        let stderr_task = tokio::spawn(capture_output(stderr));
        let status =
            if let Ok(result) = tokio::time::timeout(self.timeout, child_wait(&mut child)).await {
                result.map_err(|source| TerminalError::Execute {
                    program: self.program.clone(),
                    source,
                })?
            } else {
                terminate_owned_process(&mut child, pid).await;
                tokio::join!(
                    reap_capture_bounded(stdout_task),
                    reap_capture_bounded(stderr_task)
                );
                return Err(TerminalError::Timeout {
                    program: self.program,
                    timeout: self.timeout,
                });
            };
        let stdout = join_capture_bounded(stdout_task, &mut child, pid, &self.program).await;
        let stderr = join_capture_bounded(stderr_task, &mut child, pid, &self.program).await;
        let stdout = stdout?;
        let stderr = stderr?;
        Ok(TerminalOutput {
            status,
            stdout,
            stderr,
        })
    }
}

async fn reap_capture_bounded(mut task: tokio::task::JoinHandle<Result<String, std::io::Error>>) {
    if tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, &mut task)
        .await
        .is_err()
    {
        task.abort();
        let _ = task.await;
    }
}

async fn capture_output<R>(reader: R) -> Result<String, std::io::Error>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    reader
        .take((MAX_CAPTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_CAPTURE_BYTES {
        bytes.truncate(MAX_CAPTURE_BYTES);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn join_capture_bounded(
    mut task: tokio::task::JoinHandle<Result<String, std::io::Error>>,
    child: &mut OwnedChild,
    pid: Option<u32>,
    program: &Path,
) -> Result<String, TerminalError> {
    if let Ok(result) = tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, &mut task).await {
        result
            .map_err(|source| TerminalError::CaptureJoin {
                program: program.to_owned(),
                source,
            })?
            .map_err(|source| TerminalError::Capture {
                program: program.to_owned(),
                source,
            })
    } else {
        terminate_owned_process(child, pid).await;
        task.abort();
        let _ = task.await;
        Err(TerminalError::DescendantCleanup {
            program: program.to_owned(),
        })
    }
}

async fn terminate_owned_process(child: &mut OwnedChild, pid: Option<u32>) {
    #[cfg(not(unix))]
    let _ = pid;
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let process_group = Pid::from_raw(-pid);
        let _ = kill(process_group, Signal::SIGTERM);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = kill(process_group, Signal::SIGKILL);
    }
    let _ = child_start_kill(child);
    let _ = tokio::time::timeout(PROCESS_CLEANUP_TIMEOUT, child_wait(child)).await;
}

#[cfg(not(windows))]
fn spawn_owned(mut command: Command) -> std::io::Result<OwnedChild> {
    command.kill_on_drop(true).spawn()
}

#[cfg(windows)]
fn spawn_owned(command: Command) -> std::io::Result<OwnedChild> {
    use process_wrap::tokio::{CommandWrap, JobObject, KillOnDrop};
    CommandWrap::from(command)
        .wrap(KillOnDrop)
        .wrap(JobObject)
        .spawn()
}

fn child_id(child: &OwnedChild) -> Option<u32> {
    child.id()
}

fn child_stdout(child: &mut OwnedChild) -> Option<tokio::process::ChildStdout> {
    #[cfg(not(windows))]
    {
        child.stdout.take()
    }
    #[cfg(windows)]
    {
        process_wrap::tokio::ChildWrapper::stdout(&mut **child).take()
    }
}

fn child_stderr(child: &mut OwnedChild) -> Option<tokio::process::ChildStderr> {
    #[cfg(not(windows))]
    {
        child.stderr.take()
    }
    #[cfg(windows)]
    {
        process_wrap::tokio::ChildWrapper::stderr(&mut **child).take()
    }
}

fn child_start_kill(child: &mut OwnedChild) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        child.start_kill()
    }
    #[cfg(windows)]
    {
        process_wrap::tokio::ChildWrapper::start_kill(&mut **child)
    }
}

async fn child_wait(child: &mut OwnedChild) -> std::io::Result<ExitStatus> {
    #[cfg(not(windows))]
    {
        child.wait().await
    }
    #[cfg(windows)]
    {
        // The job wrapper's wait can use an uncancellable blocking waiter while descendants
        // drain; polling keeps the terminal timeout and cleanup futures cancellable.
        loop {
            if let Some(status) = process_wrap::tokio::ChildWrapper::try_wait(&mut **child)? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

fn expect_script(program: &Path, arguments: &[OsString], response: &TerminalResponse) -> String {
    let mut spawn = format!("spawn {}", tcl_word(program.as_os_str()));
    for argument in arguments {
        spawn.push(' ');
        spawn.push_str(&tcl_word(argument));
    }
    format!(
        concat!(
            "set timeout 5\n",
            "{}\n",
            "expect {{\n",
            "  {} {{\n",
            "    send -- \"{}\"\n",
            "    after 100\n",
            "    send -- \"\\r\"\n",
            "    exp_continue\n",
            "  }}\n",
            "  timeout {{\n",
            "    send -- \"{}\"\n",
            "    after 100\n",
            "    send -- \"\\r\"\n",
            "    exp_continue\n",
            "  }}\n",
            "  eof {{}}\n",
            "}}\n",
            "catch wait result\n",
            "exit [lindex $result 3]\n"
        ),
        spawn,
        tcl_word(OsStr::new(&response.prompt)),
        tcl_double_quoted(&response.response),
        tcl_double_quoted(&response.response),
    )
}

fn tcl_word(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    format!("{{{}}}", value.replace('{', "\\{").replace('}', "\\}"))
}

fn tcl_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('[', "\\[")
}

#[derive(Debug)]
pub struct TerminalOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl TerminalOutput {
    #[must_use]
    pub fn diagnostic(&self) -> String {
        format!(
            "status: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.status, self.stdout, self.stderr
        )
    }
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("command '{}' exceeded its {timeout:?} timeout", program.display())]
    Timeout { program: PathBuf, timeout: Duration },
    #[error("could not execute '{}': {source}", program.display())]
    Execute {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "could not capture {stream} for '{}': output pipe was unavailable",
        program.display()
    )]
    MissingOutput {
        stream: &'static str,
        program: PathBuf,
    },
    #[error("could not join output capture for '{}': {source}", program.display())]
    CaptureJoin {
        program: PathBuf,
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("could not capture output for '{}': {source}", program.display())]
    Capture {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("owned descendants of '{}' did not close their output pipes", program.display())]
    DescendantCleanup { program: PathBuf },
}

pub fn os(value: impl AsRef<OsStr>) -> OsString {
    value.as_ref().to_owned()
}

pub fn path(value: impl AsRef<Path>) -> OsString {
    value.as_ref().as_os_str().to_owned()
}

#[cfg(test)]
mod tests {
    use super::TerminalCommand;
    #[cfg(windows)]
    use std::path::{Path, PathBuf};
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(windows)]
    use std::time::Duration;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn responds_to_an_interactive_terminal_prompt() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let output = TerminalCommand::new("/bin/sh", workspace.path())
            .args([
                "-c",
                "printf prompt; read value; printf 'received:%s' \"$value\"",
            ])
            .respond_when("prompt", "answer")
            .run()
            .await
            .expect("command should complete");

        assert!(output.status.success());
        assert!(output.stdout.contains("received:answer"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_terminates_descendants_without_killing_unrelated_processes() {
        use nix::errno::Errno;
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("workspace should exist");
        let child_pid_path = workspace.path().join("child.pid");
        let script = workspace.path().join("spawn-descendant.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsh -c 'sleep 30' &\nprintf '%s' \"$!\" > '{}'\nsleep 30\n",
                child_pid_path.display()
            ),
        )
        .expect("script should be written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("script should be executable");
        let mut unrelated = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("unrelated process should start");

        let result = TerminalCommand::new(&script, workspace.path())
            .timeout(Duration::from_secs(2))
            .run()
            .await;
        assert!(result.is_err(), "the command should time out");
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .expect("descendant pid should be recorded")
            .parse::<i32>()
            .expect("descendant pid should be numeric");
        for _ in 0..20 {
            if kill(Pid::from_raw(child_pid), None).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(kill(Pid::from_raw(child_pid), None), Err(Errno::ESRCH));
        assert!(
            unrelated
                .try_wait()
                .expect("unrelated status should work")
                .is_none()
        );
        let _ = kill(
            Pid::from_raw(i32::try_from(unrelated.id()).expect("pid should fit")),
            Signal::SIGKILL,
        );
        let _ = unrelated.wait();
    }

    #[tokio::test]
    async fn captured_output_is_bounded() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let output = TerminalCommand::new("/bin/sh", workspace.path())
            .args(["-c", "head -c 100000 /dev/zero"])
            .run()
            .await
            .expect("command should complete");
        assert!(output.stdout.len() <= super::MAX_CAPTURE_BYTES);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn inherited_pipe_descendants_are_killed_with_the_owned_shell() {
        use super::TerminalError;
        use std::time::{Duration, Instant};

        if fixture_child_mode().is_some() {
            run_fixture_child_mode().await;
            return;
        }
        let workspace = tempfile::tempdir().expect("workspace should exist");
        let comspec = std::env::var_os("ComSpec")
            .or_else(|| std::env::var_os("COMSPEC"))
            .expect("Windows should provide ComSpec");
        let normal = TerminalCommand::new(&comspec, workspace.path())
            .args(["/d", "/c", "exit 0"])
            .run()
            .await
            .expect("normal shell exit should complete");
        assert!(normal.status.success());
        let fixture = workspace.path().join("fixture path with spaces");
        std::fs::create_dir(&fixture).expect("fixture directory should exist");
        let pid_file = fixture.join("leaf.pid");
        let ready_file = fixture.join("leaf.ready");
        let parent_stage_file = fixture.join("parent.started");
        let parent_launch_file = fixture.join("parent.launched");
        let leaf_stage_file = fixture.join("leaf.started");
        let mut unrelated = spawn_unrelated_process();
        let started = Instant::now();
        let current_exe = std::env::current_exe().expect("test executable should be available");
        let terminal = AbortOnDrop(Some(tokio::spawn(
            TerminalCommand::new(&current_exe, workspace.path())
                .args([
                    "--exact",
                    "terminal::tests::inherited_pipe_descendants_are_killed_with_the_owned_shell",
                    "--nocapture",
                ])
                .env("NAN_HARNESS_TERMINAL_FIXTURE_MODE", "parent")
                .env("NAN_HARNESS_TERMINAL_FIXTURE_PID", pid_file.as_os_str())
                .env("NAN_HARNESS_TERMINAL_FIXTURE_READY", ready_file.as_os_str())
                .env(
                    "NAN_HARNESS_TERMINAL_FIXTURE_PARENT_STARTED",
                    parent_stage_file.as_os_str(),
                )
                .env(
                    "NAN_HARNESS_TERMINAL_FIXTURE_PARENT_LAUNCHED",
                    parent_launch_file.as_os_str(),
                )
                .env(
                    "NAN_HARNESS_TERMINAL_FIXTURE_LEAF_STARTED",
                    leaf_stage_file.as_os_str(),
                )
                .timeout(Duration::from_secs(5))
                .run(),
        )));
        wait_for_fixture_ready(
            &ready_file,
            &pid_file,
            &parent_stage_file,
            &parent_launch_file,
            &leaf_stage_file,
        )
        .await;
        let pid = std::fs::read_to_string(&pid_file)
            .expect("descendant should publish its pid after readiness")
            .trim()
            .parse::<u32>()
            .expect("descendant pid should be numeric");
        assert!(process_is_alive(pid, &comspec).expect("liveness query should execute"));
        let result = terminal
            .join()
            .await
            .expect("terminal task should not panic");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(matches!(result, Err(TerminalError::Timeout { .. })));
        for _ in 0..20 {
            if !process_is_alive(pid, &comspec).expect("liveness query should execute") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            !process_is_alive(pid, &comspec).expect("liveness query should execute"),
            "owned descendant survived job cleanup"
        );
        assert!(
            unrelated
                .0
                .try_wait()
                .expect("unrelated status should work")
                .is_none()
        );
    }

    #[cfg(windows)]
    struct ChildGuard(std::process::Child);

    #[cfg(windows)]
    struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);

    #[cfg(windows)]
    impl<T> AbortOnDrop<T> {
        async fn join(mut self) -> Result<T, tokio::task::JoinError> {
            self.0
                .take()
                .expect("terminal task should be present")
                .await
        }
    }

    #[cfg(windows)]
    impl<T> Drop for AbortOnDrop<T> {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }

    #[cfg(windows)]
    async fn wait_for_fixture_ready(
        ready_file: &Path,
        pid_file: &Path,
        parent_stage_file: &Path,
        parent_launch_file: &Path,
        leaf_stage_file: &Path,
    ) {
        let ready = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if ready_file.is_file()
                    && pid_file.is_file()
                    && parent_stage_file.is_file()
                    && parent_launch_file.is_file()
                    && leaf_stage_file.is_file()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        assert!(
            ready.is_ok(),
            "descendant readiness handshake failed: parent={:?}, launched={:?}, leaf={:?}, pid={:?}, ready={:?}",
            std::fs::read_to_string(parent_stage_file),
            std::fs::read_to_string(parent_launch_file),
            std::fs::read_to_string(leaf_stage_file),
            std::fs::read_to_string(pid_file),
            std::fs::read_to_string(ready_file)
        );
    }

    #[cfg(windows)]
    fn fixture_child_mode() -> Option<String> {
        std::env::var("NAN_HARNESS_TERMINAL_FIXTURE_MODE").ok()
    }

    #[cfg(windows)]
    async fn run_fixture_child_mode() {
        let mode = fixture_child_mode().expect("fixture mode should be set");
        let pid_file = fixture_path("NAN_HARNESS_TERMINAL_FIXTURE_PID");
        let ready_file = fixture_path("NAN_HARNESS_TERMINAL_FIXTURE_READY");
        if mode == "leaf" {
            atomic_publish(
                &fixture_path("NAN_HARNESS_TERMINAL_FIXTURE_LEAF_STARTED"),
                "leaf-started",
            );
            atomic_publish(&pid_file, &std::process::id().to_string());
            atomic_publish(&ready_file, "ready");
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        assert_eq!(mode, "parent");
        atomic_publish(
            &fixture_path("NAN_HARNESS_TERMINAL_FIXTURE_PARENT_STARTED"),
            "parent-started",
        );
        let current_exe = std::env::current_exe().expect("test executable should be available");
        let child = ChildGuard(
            std::process::Command::new(current_exe)
                .args([
                    "--exact",
                    "terminal::tests::inherited_pipe_descendants_are_killed_with_the_owned_shell",
                    "--nocapture",
                ])
                .env("NAN_HARNESS_TERMINAL_FIXTURE_MODE", "leaf")
                .spawn()
                .expect("leaf test executable should start"),
        );
        atomic_publish(
            &fixture_path("NAN_HARNESS_TERMINAL_FIXTURE_PARENT_LAUNCHED"),
            &child.0.id().to_string(),
        );
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[cfg(windows)]
    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).expect("fixture path should be set"))
    }

    #[cfg(windows)]
    fn atomic_publish(path: &Path, contents: &str) {
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&temporary, contents).expect("fixture marker should be written");
        std::fs::rename(temporary, path).expect("fixture marker should be published atomically");
    }

    #[cfg(windows)]
    fn spawn_unrelated_process() -> ChildGuard {
        let ping = std::env::var_os("SystemRoot").map_or_else(
            || PathBuf::from("ping.exe"),
            |root| PathBuf::from(root).join("System32/ping.exe"),
        );
        ChildGuard(
            std::process::Command::new(ping)
                .args(["127.0.0.1", "-n", "100"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("unrelated process should start"),
        )
    }

    #[cfg(windows)]
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[cfg(windows)]
    fn process_is_alive(pid: u32, comspec: &std::ffi::OsStr) -> std::io::Result<bool> {
        let output = std::process::Command::new(comspec)
            .args([
                "/d",
                "/c",
                "tasklist",
                "/fi",
                &format!("PID eq {pid}"),
                "/fo",
                "csv",
                "/nh",
            ])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("liveness query failed"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")))
    }
}
