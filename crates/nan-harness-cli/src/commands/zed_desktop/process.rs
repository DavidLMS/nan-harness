use super::ZedDesktopError;
use super::paths::{ZedPlatform, current_platform};
use semver::Version;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tokio::process::{Child, Command as TokioCommand};

pub(super) struct SystemZedProcess {
    platform: ZedPlatform,
    executable: Option<PathBuf>,
}

impl SystemZedProcess {
    pub(super) fn new(executable: Option<PathBuf>) -> Result<Self, ZedDesktopError> {
        Ok(Self {
            platform: current_platform()?,
            executable,
        })
    }

    pub(super) fn ensure_available(&self) -> Result<(), ZedDesktopError> {
        if self.resolve_executable().is_some() {
            Ok(())
        } else if self.executable.is_some() {
            Err(ZedDesktopError::InvalidInstallation)
        } else {
            Err(ZedDesktopError::AppNotFound)
        }
    }

    pub(super) fn installed_version(&self) -> Result<Option<Version>, ZedDesktopError> {
        let executable = self
            .resolve_executable()
            .ok_or(ZedDesktopError::AppNotFound)?;
        let output = Command::new(executable)
            .arg("--version")
            .output()
            .map_err(ZedDesktopError::VersionCommand)?;
        if !output.status.success() {
            return Err(ZedDesktopError::VersionCommandFailed(output.status.code()));
        }
        Ok(super::extract_semver(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    pub(super) fn spawn(
        &self,
        workspace: &Path,
        arguments: &[String],
        session_token: &str,
    ) -> Result<Child, ZedDesktopError> {
        validate_passthrough_arguments(arguments)?;
        let executable = self
            .resolve_executable()
            .ok_or(ZedDesktopError::AppNotFound)?;
        let mut command = TokioCommand::new(executable);
        command
            .args(["--foreground", "--wait"])
            .args(arguments)
            .arg(workspace)
            .env_remove("NAN_API_KEY")
            .env("NAN_API_KEY", session_token)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command.spawn().map_err(ZedDesktopError::Launch)
    }

    pub(super) fn is_running(&self) -> Result<bool, ZedDesktopError> {
        match self.platform {
            ZedPlatform::Macos | ZedPlatform::Linux => unix_zed_is_running(),
            ZedPlatform::Windows => windows_zed_is_running(),
        }
    }

    pub(super) async fn terminate_and_wait(&self) -> Result<(), ZedDesktopError> {
        self.request_termination(false)?;
        for _ in 0..60 {
            if !self.is_running()? {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        self.request_termination(true)?;
        for _ in 0..40 {
            if !self.is_running()? {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Err(ZedDesktopError::DidNotTerminate)
    }

    fn resolve_executable(&self) -> Option<PathBuf> {
        if let Some(explicit) = &self.executable {
            return resolve_explicit(self.platform, explicit);
        }
        match self.platform {
            ZedPlatform::Macos => find_on_path("zed").or_else(find_macos_cli),
            ZedPlatform::Linux => find_on_path("zed").or_else(|| find_on_path("zeditor")),
            ZedPlatform::Windows => find_on_path("zed.exe").or_else(find_windows_cli),
        }
    }

    fn request_termination(&self, force: bool) -> Result<(), ZedDesktopError> {
        match self.platform {
            ZedPlatform::Macos if !force => {
                let status = Command::new("/usr/bin/osascript")
                    .args(["-e", "tell application id \"dev.zed.Zed\" to quit"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(ZedDesktopError::Terminate)?;
                if status.success() {
                    Ok(())
                } else {
                    self.terminate_by_name(false)
                }
            }
            ZedPlatform::Macos | ZedPlatform::Linux => self.terminate_by_name(force),
            ZedPlatform::Windows => {
                let mut command = Command::new("taskkill.exe");
                command.args(["/IM", "zed.exe", "/T"]);
                if force {
                    command.arg("/F");
                }
                accept_absent_process(command.status().map_err(ZedDesktopError::Terminate)?)
            }
        }
    }

    fn terminate_by_name(&self, force: bool) -> Result<(), ZedDesktopError> {
        let signal = if force { "-KILL" } else { "-TERM" };
        let names: &[&str] = match self.platform {
            ZedPlatform::Macos => &["zed"],
            ZedPlatform::Linux => &["zed", "zeditor", "zed-editor"],
            ZedPlatform::Windows => unreachable!("Windows termination uses taskkill"),
        };
        for name in names {
            let status = Command::new("pkill")
                .args([signal, "-x", name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(ZedDesktopError::Terminate)?;
            if !matches!(status.code(), Some(0 | 1)) {
                return Err(ZedDesktopError::TerminateFailed(status.code()));
            }
        }
        Ok(())
    }
}

pub(super) fn validate_passthrough_arguments(arguments: &[String]) -> Result<(), ZedDesktopError> {
    if arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--foreground" | "--wait" | "-w" | "--user-data-dir"
        ) || argument.starts_with("--user-data-dir=")
    }) {
        Err(ZedDesktopError::ReservedArgument)
    } else {
        Ok(())
    }
}

pub(super) fn resolve_explicit(platform: ZedPlatform, path: &Path) -> Option<PathBuf> {
    if platform == ZedPlatform::Macos && path.is_dir() {
        let cli = path.join("Contents/MacOS/cli");
        return is_executable(&cli).then_some(cli);
    }
    is_executable(path).then(|| path.to_path_buf())
}

pub(super) fn command_is_zed_main(command: &str) -> bool {
    if command.contains("--type=") || command.contains("/Contents/MacOS/cli") {
        return false;
    }
    if command.contains("/Zed.app/Contents/MacOS/zed") {
        return true;
    }
    let executable = command.split_whitespace().next().unwrap_or_default();
    let executable = executable.replace('\\', "/");
    Path::new(&executable)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "zed" | "zeditor" | "zed-editor" | "zed.exe"
            )
        })
}

fn unix_zed_is_running() -> Result<bool, ZedDesktopError> {
    let output = Command::new("/bin/ps")
        .args(["-ww", "-axo", "command="])
        .output()
        .map_err(ZedDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(ZedDesktopError::ProcessCheckFailed(output.status.code()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(command_is_zed_main))
}

fn windows_zed_is_running() -> Result<bool, ZedDesktopError> {
    let output = Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq zed.exe", "/FO", "CSV", "/NH"])
        .output()
        .map_err(ZedDesktopError::ProcessCheck)?;
    if !output.status.success() {
        return Err(ZedDesktopError::ProcessCheckFailed(output.status.code()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .to_ascii_lowercase()
        .contains("\"zed.exe\""))
}

fn find_macos_cli() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    [
        PathBuf::from("/Applications/Zed.app"),
        home.join("Applications/Zed.app"),
    ]
    .into_iter()
    .find_map(|app| resolve_explicit(ZedPlatform::Macos, &app))
}

fn find_windows_cli() -> Option<PathBuf> {
    let local = env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    [
        local.join("Programs/Zed/zed.exe"),
        local.join("Zed/zed.exe"),
    ]
    .into_iter()
    .find(|path| is_executable(path))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn accept_absent_process(status: std::process::ExitStatus) -> Result<(), ZedDesktopError> {
    if matches!(status.code(), Some(0 | 1 | 128)) {
        Ok(())
    } else {
        Err(ZedDesktopError::TerminateFailed(status.code()))
    }
}
