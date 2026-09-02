use super::catalog::{InstallMethod, InstallSpec};
use super::error::InstallError;
use super::post_install::refresh_cline_binary_cache;
use nan_harness_core::HarnessKind;
use std::env;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

pub(super) fn install(spec: &InstallSpec) -> Result<(), InstallError> {
    let method = spec.method()?;
    eprintln!(
        "Installing {} with the official installer...",
        spec.display_name()
    );
    match method {
        InstallMethod::ShellScript {
            url,
            interpreter,
            arguments,
        } => install_shell_script(spec.kind(), url, interpreter, arguments),
        InstallMethod::PowerShellScript { url, command } => {
            install_powershell_script(spec.kind(), url, command)
        }
        InstallMethod::Command { program, arguments } => {
            let status = Command::new(program)
                .args(arguments)
                .status()
                .map_err(|source| InstallError::CommandStart {
                    harness: spec.kind(),
                    program,
                    source,
                })?;
            if status.success() {
                if spec.kind() == HarnessKind::Cline {
                    refresh_cline_binary_cache()
                } else {
                    Ok(())
                }
            } else {
                Err(InstallError::CommandFailed {
                    harness: spec.kind(),
                    program,
                    exit_code: status.code(),
                })
            }
        }
    }
}

fn install_shell_script(
    harness: HarnessKind,
    url: &'static str,
    interpreter: &'static str,
    arguments: &[&str],
) -> Result<(), InstallError> {
    install_shell_script_with_downloader(harness, url, interpreter, arguments, |path| {
        Command::new("curl")
            .args(["-fsSL", "--output"])
            .arg(path)
            .arg(url)
            .status()
    })
}

fn install_shell_script_with_downloader(
    harness: HarnessKind,
    url: &'static str,
    interpreter: &'static str,
    arguments: &[&str],
    download: impl FnOnce(&Path) -> io::Result<std::process::ExitStatus>,
) -> Result<(), InstallError> {
    let installer = NamedTempFile::new()
        .map_err(|source| InstallError::PrepareInstaller { harness, source })?;
    let download_status =
        download(installer.path()).map_err(|source| InstallError::DownloadStart {
            harness,
            url,
            source,
        })?;
    if !download_status.success() {
        return Err(InstallError::DownloadFailed {
            harness,
            exit_code: download_status.code(),
        });
    }
    let installer_input = installer
        .reopen()
        .map_err(|source| InstallError::PrepareInstaller { harness, source })?;
    let mut installer = Command::new(interpreter);
    configure_shell_installer_command(harness, &mut installer);
    installer.arg("-s").arg("--").args(arguments);
    let installer_status = installer
        .stdin(Stdio::from(installer_input))
        .status()
        .map_err(|source| InstallError::InstallerStart {
            harness,
            interpreter,
            source,
        })?;
    if !installer_status.success() {
        return Err(InstallError::InstallerFailed {
            harness,
            interpreter,
            exit_code: installer_status.code(),
        });
    }
    Ok(())
}

fn configure_shell_installer_command(harness: HarnessKind, command: &mut Command) {
    if harness != HarnessKind::Pi {
        return;
    }
    let homebrew_bin_dir = homebrew_bin_dir();
    configure_shell_installer_path(
        harness,
        command,
        env::var_os("PATH").as_deref(),
        homebrew_bin_dir.as_deref(),
    );
}

fn configure_shell_installer_path(
    harness: HarnessKind,
    command: &mut Command,
    existing_path: Option<&OsStr>,
    homebrew_bin_dir: Option<&Path>,
) {
    if let Some(path) = preferred_installer_path(harness, existing_path, homebrew_bin_dir) {
        command.env("PATH", path);
    }
}

fn homebrew_bin_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }

    let output = Command::new("brew").arg("--prefix").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let prefix = String::from_utf8(output.stdout).ok()?;
    let prefix = PathBuf::from(prefix.trim());
    prefix.is_absolute().then(|| prefix.join("bin"))
}

fn preferred_installer_path(
    harness: HarnessKind,
    existing_path: Option<&OsStr>,
    homebrew_bin_dir: Option<&Path>,
) -> Option<std::ffi::OsString> {
    if harness != HarnessKind::Pi {
        return None;
    }
    let existing_path = existing_path?;
    if existing_path.is_empty() {
        return None;
    }
    let homebrew_bin_dir = homebrew_bin_dir?;
    let current_paths = env::split_paths(existing_path).collect::<Vec<_>>();
    let mut preferred_paths = Vec::with_capacity(current_paths.len() + 1);
    preferred_paths.push(homebrew_bin_dir.to_path_buf());
    preferred_paths.extend(
        current_paths
            .iter()
            .filter(|path| path.as_path() != homebrew_bin_dir)
            .cloned(),
    );
    if preferred_paths == current_paths {
        return None;
    }
    env::join_paths(preferred_paths).ok()
}

fn install_powershell_script(
    harness: HarnessKind,
    _url: &'static str,
    command: &str,
) -> Result<(), InstallError> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ])
        .status()
        .map_err(|source| InstallError::InstallerStart {
            harness,
            interpreter: "powershell",
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::InstallerFailed {
            harness,
            interpreter: "powershell",
            exit_code: status.code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        configure_shell_installer_path, install_powershell_script,
        install_shell_script_with_downloader, preferred_installer_path,
    };
    use crate::commands::install::catalog::{KIMI_CODE_INSTALL_URL, command};
    use nan_harness_core::HarnessKind;
    use std::fs;

    #[test]
    fn pi_installer_prefers_homebrew_over_a_version_manager() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");
        let configured_path = preferred_installer_path(
            HarnessKind::Pi,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        )
        .expect("Pi should receive a Homebrew-preferred PATH");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("expected PATH should be valid");

        assert_eq!(configured_path, expected_path);
    }

    #[test]
    fn pi_installer_removes_duplicate_homebrew_entries() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/opt/homebrew/bin"),
        ])
        .expect("test PATH should be valid");
        let configured_path = preferred_installer_path(
            HarnessKind::Pi,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        )
        .expect("duplicate Homebrew entries should be normalized");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
        ])
        .expect("expected PATH should be valid");

        assert_eq!(configured_path, expected_path);
    }

    #[test]
    fn pi_installer_leaves_an_already_preferred_path_unchanged() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");

        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                Some(&current_path),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                Some(std::ffi::OsStr::new("")),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
    }

    #[test]
    fn only_pi_gets_the_homebrew_installer_path() {
        let current_path = std::env::join_paths([std::path::Path::new("/usr/bin")])
            .expect("test PATH should be valid");

        assert_eq!(
            preferred_installer_path(
                HarnessKind::ClaudeCode,
                Some(&current_path),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
        assert_eq!(
            preferred_installer_path(HarnessKind::Pi, Some(&current_path), None),
            None
        );
        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                None,
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
    }

    #[test]
    fn pi_installer_command_receives_the_preferred_path() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("expected PATH should be valid");
        let mut command = std::process::Command::new("sh");

        configure_shell_installer_path(
            HarnessKind::Pi,
            &mut command,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        );

        let configured_path = command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new("PATH"))
            .and_then(|(_, value)| value);
        assert_eq!(configured_path, Some(expected_path.as_os_str()));
    }

    #[cfg(unix)]
    #[test]
    fn failed_installer_command_is_reported_with_exit_code() {
        let spec = super::InstallSpec {
            kind: HarnessKind::KimiCode,
            display_name: "Kimi Code",
            official_url: KIMI_CODE_INSTALL_URL,
            unix: command("sh", &["-c", "exit 23"]),
            windows: None,
        };

        let error = super::install(&spec).expect_err("failed installer should be reported");
        assert!(matches!(
            error,
            super::InstallError::CommandFailed {
                harness: HarnessKind::KimiCode,
                program: "sh",
                exit_code: Some(23),
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unavailable_installer_interpreter_is_reported_as_start_error() {
        let error = install_powershell_script(
            HarnessKind::KimiCode,
            "https://code.kimi.com/kimi-code/install.ps1",
            "exit",
        )
        .expect_err("missing powershell should be reported");
        assert!(matches!(
            error,
            super::InstallError::InstallerStart {
                harness: HarnessKind::KimiCode,
                interpreter: "powershell",
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn failed_downloads_never_execute_partial_installers() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let marker = directory.path().join("executed");
        let error = install_shell_script_with_downloader(
            HarnessKind::KimiCode,
            KIMI_CODE_INSTALL_URL,
            "sh",
            &[],
            |path| {
                fs::write(path, format!("touch '{}'\n", marker.display()))?;
                std::process::Command::new("sh")
                    .args(["-c", "exit 23"])
                    .status()
            },
        )
        .expect_err("failed download should be reported");

        assert!(matches!(
            error,
            super::InstallError::DownloadFailed {
                harness: HarnessKind::KimiCode,
                exit_code: Some(23),
            }
        ));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn completed_downloads_execute_the_buffered_installer() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let marker = directory.path().join("executed");
        install_shell_script_with_downloader(
            HarnessKind::KimiCode,
            KIMI_CODE_INSTALL_URL,
            "sh",
            &[],
            |path| {
                fs::write(path, format!("printf INSTALLED > '{}'\n", marker.display()))?;
                std::process::Command::new("sh")
                    .args(["-c", "exit 0"])
                    .status()
            },
        )
        .expect("completed download should execute");

        assert_eq!(
            fs::read_to_string(marker).expect("installer marker should exist"),
            "INSTALLED"
        );
    }
}
