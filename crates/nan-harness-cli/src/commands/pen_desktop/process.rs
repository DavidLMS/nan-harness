use super::PenDesktopError;
use super::paths::user_home;
use semver::Version;
use std::env;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WaitOutcome {
    Exited,
    Signaled(i32),
}

pub(super) async fn wait_for_exit_or_signal(
    process: &SystemPenProcess,
) -> Result<WaitOutcome, PenDesktopError> {
    let mut observed_running = false;
    let mut startup_polls = 0_u8;
    let signal = termination_signal();
    tokio::pin!(signal);
    loop {
        if process.is_running()? {
            observed_running = true;
        } else if observed_running {
            return Ok(WaitOutcome::Exited);
        } else {
            startup_polls = startup_polls.saturating_add(1);
            if startup_polls >= 40 {
                return Err(PenDesktopError::DidNotStart);
            }
        }
        if let Some(code) = wait_for_poll_or_signal(signal.as_mut()).await {
            return Ok(WaitOutcome::Signaled(code));
        }
    }
}

async fn wait_for_poll_or_signal<F>(signal: std::pin::Pin<&mut F>) -> Option<i32>
where
    F: Future<Output = i32>,
{
    tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(125)) => None,
        code = signal => Some(code),
    }
}

pub(super) async fn terminate_and_wait(process: &SystemPenProcess) -> Result<(), PenDesktopError> {
    let _ = process.terminate(false);
    for _ in 0..120 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    process.terminate(true)?;
    for _ in 0..40 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    Err(PenDesktopError::DidNotTerminate)
}

#[cfg(unix)]
async fn termination_signal() -> i32 {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return 130;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => 130,
        _ = terminate.recv() => 143,
    }
}

#[cfg(not(unix))]
async fn termination_signal() -> i32 {
    let _ = tokio::signal::ctrl_c().await;
    130
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PenPlatform {
    Macos,
    Windows,
    Linux,
}

impl PenPlatform {
    fn current() -> Result<Self, PenDesktopError> {
        if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(windows) {
            Ok(Self::Windows)
        } else if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else {
            Err(PenDesktopError::UnsupportedPlatform)
        }
    }
}

pub(super) struct SystemPenProcess {
    platform: PenPlatform,
    executable: Option<PathBuf>,
}

impl SystemPenProcess {
    pub(super) fn new(executable: Option<PathBuf>) -> Result<Self, PenDesktopError> {
        Ok(Self {
            platform: PenPlatform::current()?,
            executable,
        })
    }

    pub(super) fn ensure_available(&self) -> Result<(), PenDesktopError> {
        if self.resolve_executable().is_some() {
            Ok(())
        } else {
            Err(PenDesktopError::AppNotFound)
        }
    }

    fn resolve_executable(&self) -> Option<PathBuf> {
        if let Some(explicit) = &self.executable {
            if self.platform == PenPlatform::Macos && explicit.is_dir() {
                let executable = explicit.join("Contents/MacOS/Pen");
                return executable.is_file().then_some(executable);
            }
            return explicit.is_file().then(|| explicit.clone());
        }
        match self.platform {
            PenPlatform::Macos => find_macos_app().map(|app| app.join("Contents/MacOS/Pen")),
            PenPlatform::Windows => find_windows_app(),
            PenPlatform::Linux => find_on_path("pen").or_else(|| find_on_path("Pen")),
        }
    }

    pub(super) fn installed_version(&self) -> Option<Version> {
        if self.platform != PenPlatform::Macos {
            return None;
        }
        let executable = self.resolve_executable()?;
        let app = executable.parent()?.parent()?.parent()?;
        let output = Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist"))
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| super::extract_semver(&String::from_utf8_lossy(&output.stdout)))
            .flatten()
    }

    pub(super) fn launch(&self) -> Result<(), PenDesktopError> {
        let executable = self
            .resolve_executable()
            .ok_or(PenDesktopError::AppNotFound)?;
        let mut command = if self.platform == PenPlatform::Macos {
            let app = executable
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or(PenDesktopError::InvalidInstallation)?;
            let mut command = Command::new("/usr/bin/open");
            command.arg(app);
            command
        } else {
            Command::new(executable)
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(PenDesktopError::Launch)
    }

    pub(super) fn is_running(&self) -> Result<bool, PenDesktopError> {
        match self.platform {
            PenPlatform::Macos => {
                process_matches("/usr/bin/pgrep", &["-f", "Pen.app/Contents/MacOS/Pen"])
            }
            PenPlatform::Linux => Ok(process_matches("pgrep", &["-x", "Pen"])?
                || process_matches("pgrep", &["-x", "pen"])?),
            PenPlatform::Windows => {
                let output = Command::new("tasklist.exe")
                    .args(["/FI", "IMAGENAME eq Pen.exe", "/FO", "CSV", "/NH"])
                    .output()
                    .map_err(PenDesktopError::ProcessCheck)?;
                if !output.status.success() {
                    return Err(PenDesktopError::ProcessCheckFailed(output.status.code()));
                }
                Ok(String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains("\"pen.exe\""))
            }
        }
    }

    pub(super) fn terminate(&self, force: bool) -> Result<(), PenDesktopError> {
        if self.platform == PenPlatform::Linux {
            let signal = if force { "-KILL" } else { "-TERM" };
            for process_name in ["Pen", "pen"] {
                let status = Command::new("pkill")
                    .args([signal, "-x", process_name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(PenDesktopError::Terminate)?;
                if !matches!(status.code(), Some(0 | 1)) {
                    return Err(PenDesktopError::TerminateFailed(status.code()));
                }
            }
            return Ok(());
        }
        let (command, arguments): (&str, Vec<&str>) = match self.platform {
            PenPlatform::Macos => (
                "/usr/bin/pkill",
                if force {
                    vec!["-KILL", "-f", "Pen.app/Contents/MacOS/Pen"]
                } else {
                    vec!["-TERM", "-f", "Pen.app/Contents/MacOS/Pen"]
                },
            ),
            PenPlatform::Linux => unreachable!("Linux termination returns above"),
            PenPlatform::Windows => (
                "taskkill.exe",
                if force {
                    vec!["/F", "/IM", "Pen.exe", "/T"]
                } else {
                    vec!["/IM", "Pen.exe", "/T"]
                },
            ),
        };
        let status = Command::new(command)
            .args(arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(PenDesktopError::Terminate)?;
        if matches!(status.code(), Some(0 | 1)) {
            Ok(())
        } else {
            Err(PenDesktopError::TerminateFailed(status.code()))
        }
    }
}

fn process_matches(command: &str, arguments: &[&str]) -> Result<bool, PenDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(PenDesktopError::ProcessCheck)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(PenDesktopError::ProcessCheckFailed(code)),
    }
}

fn find_macos_app() -> Option<PathBuf> {
    let home = user_home()?;
    [
        PathBuf::from("/Applications/Pen.app"),
        home.join("Applications/Pen.app"),
    ]
    .into_iter()
    .find(|path| path.join("Contents/MacOS/Pen").is_file())
}

fn find_windows_app() -> Option<PathBuf> {
    let local = env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    [
        local.join("Programs/Pen/Pen.exe"),
        local.join("Pen/Pen.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")?
        .to_string_lossy()
        .split(if cfg!(windows) { ';' } else { ':' })
        .map(Path::new)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}
