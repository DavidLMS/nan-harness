use super::ChatGptDesktopError;
use super::installation::ChatGptInstallation;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux", test))]
use super::installation::parse_version_output;
use crate::commands::desktop::reject_symlink;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[cfg(target_os = "macos")]
use std::ffi::OsStr;

#[cfg(target_os = "macos")]
const APP_BUNDLE_ID: &str = "com.openai.codex";

pub(super) fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
    process_platform::chatgpt_is_running()
}

#[cfg(all(target_os = "linux", test))]
pub(super) fn is_chatgpt_app_root(candidate: &Path) -> bool {
    platform_discovery::is_chatgpt_app_root(candidate)
}

#[cfg(target_os = "macos")]
mod process_platform {
    use super::{ChatGptDesktopError, Stdio};

    pub(super) fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
        let status = std::process::Command::new("/usr/bin/pgrep")
            .args(["-x", "ChatGPT"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(ChatGptDesktopError::InspectProcess)?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(ChatGptDesktopError::ProcessInspectionFailed),
        }
    }
}

#[cfg(target_os = "windows")]
mod process_platform {
    use super::ChatGptDesktopError;

    pub(super) fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
        let output = std::process::Command::new("tasklist.exe")
            .args(["/FI", "IMAGENAME eq ChatGPT.exe", "/FO", "CSV", "/NH"])
            .output()
            .map_err(ChatGptDesktopError::InspectProcess)?;
        if !output.status.success() {
            return Err(ChatGptDesktopError::ProcessInspectionFailed);
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains("\"ChatGPT.exe\""))
    }
}

#[cfg(target_os = "linux")]
mod process_platform {
    use super::{ChatGptDesktopError, Stdio};

    pub(super) fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
        let status = std::process::Command::new("pgrep")
            .args(["-x", "ChatGPT"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(ChatGptDesktopError::InspectProcess)?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(ChatGptDesktopError::ProcessInspectionFailed),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod process_platform {
    use super::ChatGptDesktopError;

    pub(super) fn chatgpt_is_running() -> Result<bool, ChatGptDesktopError> {
        Err(ChatGptDesktopError::UnsupportedPlatform)
    }
}

pub(super) fn discover_installation(
    explicit: Option<&Path>,
) -> Result<ChatGptInstallation, ChatGptDesktopError> {
    platform_discovery::discover_installation(explicit)
}

#[cfg(target_os = "macos")]
mod platform_discovery {
    use super::{
        APP_BUNDLE_ID, ChatGptDesktopError, ChatGptInstallation, OsStr, Path, PathBuf,
        parse_version_output, reject_symlink,
    };

    pub(super) fn discover_installation(
        explicit: Option<&Path>,
    ) -> Result<ChatGptInstallation, ChatGptDesktopError> {
        let candidates = if let Some(path) = explicit {
            let application = path
                .ancestors()
                .find(|candidate| candidate.extension() == Some(OsStr::new("app")))
                .map(Path::to_path_buf)
                .ok_or(ChatGptDesktopError::InvalidInstallation)?;
            vec![application]
        } else {
            let mut candidates = vec![PathBuf::from("/Applications/ChatGPT.app")];
            if let Some(home) = std::env::var_os("HOME") {
                candidates.push(PathBuf::from(home).join("Applications/ChatGPT.app"));
            }
            candidates
        };
        let application = candidates
            .into_iter()
            .find(|candidate| {
                candidate.is_dir() && candidate.extension() == Some(OsStr::new("app"))
            })
            .ok_or(ChatGptDesktopError::AppNotFound)?;
        reject_symlink(&application).map_err(ChatGptDesktopError::from)?;
        let executable = application.join("Contents/MacOS/ChatGPT");
        let bundled_codex = application.join("Contents/Resources/codex");
        let info_plist = application.join("Contents/Info.plist");
        if !executable.is_file() || !bundled_codex.is_file() || !info_plist.is_file() {
            return Err(ChatGptDesktopError::InvalidInstallation);
        }
        let bundle_output = std::process::Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&info_plist)
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !bundle_output.status.success()
            || String::from_utf8_lossy(&bundle_output.stdout).trim() != APP_BUNDLE_ID
        {
            return Err(ChatGptDesktopError::InvalidInstallation);
        }
        let app_output = std::process::Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(&info_plist)
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !app_output.status.success() {
            return Err(ChatGptDesktopError::VersionCommandFailed);
        }
        let app_version = parse_version_output(&String::from_utf8_lossy(&app_output.stdout))?;
        let codex_output = std::process::Command::new(&bundled_codex)
            .arg("--version")
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !codex_output.status.success() {
            return Err(ChatGptDesktopError::VersionCommandFailed);
        }
        let bundled_codex_version =
            parse_version_output(&String::from_utf8_lossy(&codex_output.stdout))?;
        Ok(ChatGptInstallation {
            executable,
            app_version,
            bundled_codex_version,
        })
    }
}

#[cfg(target_os = "windows")]
mod platform_discovery {
    use super::{
        ChatGptDesktopError, ChatGptInstallation, Path, PathBuf, parse_version_output,
        reject_symlink,
    };
    use semver::Version;
    use std::fs;

    pub(super) fn discover_installation(
        explicit: Option<&Path>,
    ) -> Result<ChatGptInstallation, ChatGptDesktopError> {
        let executable = if let Some(executable) = explicit {
            executable.to_path_buf()
        } else {
            let program_files = std::env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .ok_or(ChatGptDesktopError::AppNotFound)?;
            let packages = program_files.join("WindowsApps");
            let mut roots = fs::read_dir(packages)
                .map_err(ChatGptDesktopError::InspectProfile)?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("OpenAI.ChatGPT_"))
                })
                .collect::<Vec<_>>();
            roots.sort_unstable();
            roots
                .into_iter()
                .rev()
                .find_map(|root| {
                    [
                        root.join("app/ChatGPT.exe"),
                        root.join("ChatGPT.exe"),
                        root.join("ChatGPT/ChatGPT.exe"),
                    ]
                    .into_iter()
                    .find(|path| path.is_file())
                })
                .ok_or(ChatGptDesktopError::AppNotFound)?
        };
        let package_root = executable
            .ancestors()
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("OpenAI.ChatGPT_"))
            })
            .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        if executable.file_name().and_then(|name| name.to_str()) != Some("ChatGPT.exe") {
            return Err(ChatGptDesktopError::InvalidInstallation);
        }
        let package_name = package_root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ChatGptDesktopError::UnparseableVersion)?;
        let app_version = package_name
            .split('_')
            .nth(1)
            .and_then(|version| Version::parse(version).ok())
            .ok_or(ChatGptDesktopError::UnparseableVersion)?;
        let bundled_codex = [
            package_root.join("app/resources/codex.exe"),
            package_root.join("resources/codex.exe"),
            package_root.join("codex.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        let codex_output = std::process::Command::new(bundled_codex)
            .arg("--version")
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !codex_output.status.success() {
            return Err(ChatGptDesktopError::VersionCommandFailed);
        }
        let bundled_codex_version =
            parse_version_output(&String::from_utf8_lossy(&codex_output.stdout))?;
        Ok(ChatGptInstallation {
            executable,
            app_version,
            bundled_codex_version,
        })
    }
}

#[cfg(target_os = "linux")]
mod platform_discovery {
    use super::{
        ChatGptDesktopError, ChatGptInstallation, Path, PathBuf, parse_version_output,
        reject_symlink,
    };
    use std::fs;

    const APP_DIRECTORY: &str = "/usr/lib/chatgpt";
    const APP_LAUNCHER: &str = "/usr/bin/chatgpt";

    pub(super) fn discover_installation(
        explicit: Option<&Path>,
    ) -> Result<ChatGptInstallation, ChatGptDesktopError> {
        let app_root = if let Some(path) = explicit {
            let resolved = fs::canonicalize(path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ChatGptDesktopError::AppNotFound
                } else {
                    ChatGptDesktopError::InvalidInstallation
                }
            })?;
            resolved
                .ancestors()
                .find(|candidate| is_chatgpt_app_root(candidate))
                .map(Path::to_path_buf)
                .ok_or(ChatGptDesktopError::InvalidInstallation)?
        } else {
            let mut candidates = vec![PathBuf::from(APP_DIRECTORY)];
            if let Ok(launcher) = fs::canonicalize(APP_LAUNCHER)
                && let Some(parent) = launcher.parent()
            {
                candidates.push(parent.to_path_buf());
            }
            candidates
                .into_iter()
                .find(|candidate| is_chatgpt_app_root(candidate))
                .ok_or(ChatGptDesktopError::AppNotFound)?
        };
        reject_symlink(&app_root).map_err(ChatGptDesktopError::from)?;
        build_installation(&app_root)
    }

    pub(super) fn is_chatgpt_app_root(candidate: &Path) -> bool {
        candidate.is_dir()
            && candidate.join("ChatGPT").is_file()
            && candidate.join("resources/codex").is_file()
    }

    fn build_installation(app_root: &Path) -> Result<ChatGptInstallation, ChatGptDesktopError> {
        let executable = app_root.join("ChatGPT");
        let bundled_codex = app_root.join("resources/codex");
        let app_output = std::process::Command::new(&executable)
            .arg("--version")
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !app_output.status.success() {
            return Err(ChatGptDesktopError::VersionCommandFailed);
        }
        let app_version = parse_version_output(&String::from_utf8_lossy(&app_output.stdout))?;
        let codex_output = std::process::Command::new(&bundled_codex)
            .arg("--version")
            .output()
            .map_err(ChatGptDesktopError::VersionCommand)?;
        if !codex_output.status.success() {
            return Err(ChatGptDesktopError::VersionCommandFailed);
        }
        let bundled_codex_version =
            parse_version_output(&String::from_utf8_lossy(&codex_output.stdout))?;
        Ok(ChatGptInstallation {
            executable,
            app_version,
            bundled_codex_version,
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform_discovery {
    use super::{ChatGptDesktopError, ChatGptInstallation, Path};

    pub(super) fn discover_installation(
        _explicit: Option<&Path>,
    ) -> Result<ChatGptInstallation, ChatGptDesktopError> {
        Err(ChatGptDesktopError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "macos")]
pub(super) async fn request_quit() {
    let _ = tokio::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            &format!("tell application id \"{APP_BUNDLE_ID}\" to quit"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

#[cfg(not(target_os = "macos"))]
pub(super) async fn request_quit() {}
