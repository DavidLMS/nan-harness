#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) trait DesktopProcess {
    fn is_running(&self) -> Result<bool, ClaudeDesktopError>;
    fn ensure_available(&self) -> Result<(), ClaudeDesktopError>;
    fn launch(&self) -> Result<(), ClaudeDesktopError>;
    fn terminate(&self) -> Result<(), ClaudeDesktopError>;
    fn force_terminate(&self) -> Result<(), ClaudeDesktopError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DesktopPlatform {
    Macos,
    Linux,
    Windows,
}

impl DesktopPlatform {
    pub(super) fn current() -> Result<Self, ClaudeDesktopError> {
        if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "windows") {
            Ok(Self::Windows)
        } else {
            Err(ClaudeDesktopError::UnsupportedPlatform)
        }
    }

    const fn installation_hint(self) -> &'static str {
        match self {
            Self::Macos => "macOS (/Applications or ~/Applications)",
            Self::Linux => "Linux (`claude-desktop` on PATH)",
            Self::Windows => "Windows (registered `claude://` handler or per-user installation)",
        }
    }
}

pub(super) struct SystemDesktopProcess {
    platform: DesktopPlatform,
    executable: Option<PathBuf>,
}

impl SystemDesktopProcess {
    pub(super) const fn new(platform: DesktopPlatform, executable: Option<PathBuf>) -> Self {
        Self {
            platform,
            executable,
        }
    }
}

impl DesktopProcess for SystemDesktopProcess {
    fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => process_matches(
                "/usr/bin/pgrep",
                &["-f", "Claude.app/Contents/MacOS/Claude"],
            ),
            DesktopPlatform::Linux => linux_desktop_running(),
            DesktopPlatform::Windows => windows_desktop_running(),
        }
    }

    fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
        if let Some(executable) = &self.executable {
            if is_executable_file(executable)
                || (self.platform == DesktopPlatform::Macos
                    && executable.is_dir()
                    && executable.extension().is_some_and(|value| value == "app"))
            {
                return Ok(());
            }
            return Err(ClaudeDesktopError::AppNotFound {
                platform: self.platform.installation_hint(),
            });
        }
        let available = match self.platform {
            DesktopPlatform::Macos => find_macos_app().is_some(),
            DesktopPlatform::Linux => find_executable("claude-desktop").is_some(),
            DesktopPlatform::Windows => {
                find_windows_app().is_some() || windows_protocol_registered()?
            }
        };
        if available {
            Ok(())
        } else {
            Err(ClaudeDesktopError::AppNotFound {
                platform: self.platform.installation_hint(),
            })
        }
    }

    fn launch(&self) -> Result<(), ClaudeDesktopError> {
        if let Some(executable) = &self.executable {
            if self.platform == DesktopPlatform::Macos && executable.is_dir() {
                return run_launcher("/usr/bin/open", &[executable.as_os_str()]);
            }
            return Command::new(executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(ClaudeDesktopError::Launch);
        }
        match self.platform {
            DesktopPlatform::Macos => {
                let app = find_macos_app().ok_or(ClaudeDesktopError::AppNotFound {
                    platform: self.platform.installation_hint(),
                })?;
                run_launcher("/usr/bin/open", &[app.as_os_str()])
            }
            DesktopPlatform::Linux => {
                let executable =
                    find_executable("claude-desktop").ok_or(ClaudeDesktopError::AppNotFound {
                        platform: self.platform.installation_hint(),
                    })?;
                Command::new(executable)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map(|_| ())
                    .map_err(ClaudeDesktopError::Launch)
            }
            DesktopPlatform::Windows => {
                if let Some(executable) = find_windows_app() {
                    Command::new(executable)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map(|_| ())
                        .map_err(ClaudeDesktopError::Launch)
                } else {
                    run_launcher(
                        "explorer.exe",
                        &[std::ffi::OsStr::new("claude://claude.ai/new")],
                    )
                }
            }
        }
    }

    fn terminate(&self) -> Result<(), ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => terminate_macos(),
            DesktopPlatform::Linux => terminate_linux(LinuxSignal::Terminate),
            DesktopPlatform::Windows => {
                terminate_matches("taskkill.exe", &["/IM", "Claude.exe", "/T"])
            }
        }
    }

    fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
        match self.platform {
            DesktopPlatform::Macos => terminate_matches(
                "/usr/bin/pkill",
                &["-KILL", "-f", "Claude.app/Contents/MacOS/Claude"],
            ),
            DesktopPlatform::Linux => terminate_linux(LinuxSignal::Kill),
            DesktopPlatform::Windows => {
                terminate_matches("taskkill.exe", &["/F", "/IM", "Claude.exe", "/T"])
            }
        }
    }
}

fn process_matches(command: &str, arguments: &[&str]) -> Result<bool, ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::ProcessCheck)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(ClaudeDesktopError::ProcessCheckFailed(status.code())),
    }
}

#[derive(Debug, Clone, Copy)]
enum LinuxSignal {
    Terminate,
    Kill,
}

#[cfg(target_os = "linux")]
fn linux_desktop_running() -> Result<bool, ClaudeDesktopError> {
    linux_desktop_pids().map(|process_ids| !process_ids.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn linux_desktop_running() -> Result<bool, ClaudeDesktopError> {
    Err(ClaudeDesktopError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn linux_desktop_pids() -> Result<Vec<nix::unistd::Pid>, ClaudeDesktopError> {
    let entries = fs::read_dir("/proc").map_err(ClaudeDesktopError::ProcessCheck)?;
    Ok(entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let process_id = entry.file_name().to_string_lossy().parse::<i32>().ok()?;
            let command = fs::read_to_string(entry.path().join("comm")).ok()?;
            (command.trim() == "claude-desktop").then(|| nix::unistd::Pid::from_raw(process_id))
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn terminate_linux(signal: LinuxSignal) -> Result<(), ClaudeDesktopError> {
    let signal = match signal {
        LinuxSignal::Terminate => nix::sys::signal::Signal::SIGTERM,
        LinuxSignal::Kill => nix::sys::signal::Signal::SIGKILL,
    };
    for process_id in linux_desktop_pids()? {
        if let Err(error) = nix::sys::signal::kill(process_id, signal) {
            if error == nix::errno::Errno::ESRCH {
                continue;
            }
            return Err(ClaudeDesktopError::Terminate(
                std::io::Error::from_raw_os_error(error as i32),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn terminate_linux(_signal: LinuxSignal) -> Result<(), ClaudeDesktopError> {
    Err(ClaudeDesktopError::UnsupportedPlatform)
}

fn windows_desktop_running() -> Result<bool, ClaudeDesktopError> {
    let output = Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq Claude.exe", "/FO", "CSV", "/NH"])
        .output()
        .map_err(ClaudeDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(ClaudeDesktopError::ProcessCheckFailed(output.status.code()));
    }
    Ok(tasklist_reports_desktop(&output.stdout))
}

pub(super) fn tasklist_reports_desktop(output: &[u8]) -> bool {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.trim_start().split(',').next())
        .map(|image_name| image_name.trim_matches('"'))
        .any(|image_name| image_name.eq_ignore_ascii_case("Claude.exe"))
}

fn run_launcher(command: &str, arguments: &[&std::ffi::OsStr]) -> Result<(), ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .status()
        .map_err(ClaudeDesktopError::Launch)?;
    if status.success() {
        Ok(())
    } else {
        Err(ClaudeDesktopError::LaunchFailed(status.code()))
    }
}

fn terminate_matches(command: &str, arguments: &[&str]) -> Result<(), ClaudeDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::Terminate)?;
    if matches!(status.code(), Some(0 | 1 | 128)) {
        Ok(())
    } else {
        Err(ClaudeDesktopError::TerminateFailed(status.code()))
    }
}

fn terminate_macos() -> Result<(), ClaudeDesktopError> {
    let graceful = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "tell application id \"com.anthropic.claudefordesktop\" to quit",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(ClaudeDesktopError::Terminate)?;
    if graceful.success() {
        return Ok(());
    }
    terminate_matches(
        "/usr/bin/pkill",
        &["-TERM", "-f", "Claude.app/Contents/MacOS/Claude"],
    )
}

fn find_macos_app() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/Claude.app")];
    if let Some(home) = user_home_directory() {
        candidates.push(home.join("Applications/Claude.app"));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| is_executable_file(path))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_windows_app() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidates = [
        local.join("Programs/Claude/Claude.exe"),
        local.join("Programs/Claude Desktop/Claude.exe"),
        local.join("Claude/Claude.exe"),
        local.join("Claude Desktop/Claude.exe"),
        local.join("AnthropicClaude/Claude.exe"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| {
            find_versioned_windows_app(&local.join("AnthropicClaude"))
                .or_else(|| find_versioned_windows_app(&local.join("Programs/Claude")))
                .or_else(|| find_versioned_windows_app(&local.join("Programs/Claude Desktop")))
        })
}

fn find_versioned_windows_app(root: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("app-"))
        .map(|entry| entry.path().join("Claude.exe"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.pop()
}

fn windows_protocol_registered() -> Result<bool, ClaudeDesktopError> {
    for key in [
        r"HKCU\Software\Classes\claude\shell\open\command",
        r"HKCR\claude\shell\open\command",
    ] {
        let status = Command::new("reg.exe")
            .args(["query", key, "/ve"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(ClaudeDesktopError::ProcessCheck)?;
        match status.code() {
            Some(0) => return Ok(true),
            Some(1) => {}
            _ => return Err(ClaudeDesktopError::ProcessCheckFailed(status.code())),
        }
    }
    Ok(false)
}
