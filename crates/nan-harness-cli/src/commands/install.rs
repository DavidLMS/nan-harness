use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;

const KIMI_CODE_INSTALL_URL: &str = "https://code.kimi.com/kimi-code/install.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KimiInstallDecision {
    NotInteractive,
    Declined,
    Installed,
}

pub(crate) fn offer_kimi_code_install() -> Result<KimiInstallDecision, InstallError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(KimiInstallDecision::NotInteractive);
    }

    let mut input = io::stdin().lock();
    let mut output = io::stderr().lock();
    writeln!(
        output,
        "Kimi Code was not found. Install the latest official release now?"
    )
    .map_err(InstallError::Prompt)?;
    writeln!(
        output,
        "Official installer: curl -fsSL {KIMI_CODE_INSTALL_URL} | bash"
    )
    .map_err(InstallError::Prompt)?;
    write!(output, "Install Kimi Code [y/N]: ").map_err(InstallError::Prompt)?;
    output.flush().map_err(InstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(InstallError::Prompt)?;
    if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        return Ok(KimiInstallDecision::Declined);
    }

    install_kimi_code()?;
    Ok(KimiInstallDecision::Installed)
}

pub(crate) fn kimi_code_executable_from_known_locations() -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    find_kimi_code_executable(&PathBuf::from(home))
}

fn find_kimi_code_executable(home: &Path) -> Option<PathBuf> {
    kimi_code_executable_candidates(home)
        .into_iter()
        .find(|executable| fs::metadata(executable).is_ok_and(|metadata| metadata.is_file()))
}

fn kimi_code_executable_candidates(home: &Path) -> Vec<PathBuf> {
    let executable_name = if cfg!(windows) { "kimi.exe" } else { "kimi" };
    vec![
        home.join(".kimi-code/bin").join(executable_name),
        home.join(".local/bin").join(executable_name),
    ]
}

fn install_kimi_code() -> Result<(), InstallError> {
    eprintln!("Installing Kimi Code with the official installer...");

    #[cfg(unix)]
    {
        let mut download = Command::new("curl")
            .args(["-fsSL", KIMI_CODE_INSTALL_URL])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(InstallError::Download)?;
        let installer_input = download
            .stdout
            .take()
            .ok_or(InstallError::MissingInstallerInput)?;
        let installer_status = Command::new("bash")
            .stdin(installer_input)
            .status()
            .map_err(InstallError::Installer)?;
        let download_status = download.wait().map_err(InstallError::Download)?;

        if !download_status.success() {
            return Err(InstallError::DownloadFailed(download_status.code()));
        }
        if !installer_status.success() {
            return Err(InstallError::InstallerFailed(installer_status.code()));
        }
    }

    #[cfg(windows)]
    {
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
            ])
            .status()
            .map_err(InstallError::Installer)?;
        if !status.success() {
            return Err(InstallError::InstallerFailed(status.code()));
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum InstallError {
    #[error("could not prompt for Kimi Code installation: {0}")]
    Prompt(io::Error),
    #[error("could not start the Kimi Code download with curl: {0}")]
    Download(io::Error),
    #[error("the Kimi Code installer download did not produce input for bash")]
    MissingInstallerInput,
    #[error("could not start the Kimi Code installer: {0}")]
    Installer(io::Error),
    #[error("the Kimi Code download failed{}", .0.map_or_else(String::new, |code| format!(" with exit code {code}")))]
    DownloadFailed(Option<i32>),
    #[error("the Kimi Code installer failed{}", .0.map_or_else(String::new, |code| format!(" with exit code {code}")))]
    InstallerFailed(Option<i32>),
}

impl InstallError {
    pub(crate) const fn code() -> &'static str {
        "NH-INSTALL-001"
    }
}

#[cfg(test)]
mod tests {
    use super::{KIMI_CODE_INSTALL_URL, find_kimi_code_executable};
    use std::fs;

    #[test]
    fn official_kimi_install_url_is_the_current_code_installer() {
        assert_eq!(
            KIMI_CODE_INSTALL_URL,
            "https://code.kimi.com/kimi-code/install.sh"
        );
    }

    #[test]
    fn finds_the_current_kimi_code_install_location() {
        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = directory.path().join(".kimi-code/bin/kimi");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("Kimi Code bin directory should be created");
        fs::write(&executable, "fake kimi executable").expect("fake executable should be written");

        assert_eq!(
            find_kimi_code_executable(directory.path()),
            Some(executable)
        );
    }
}
