use super::UninstallError;
use super::installation::InstallationPaths;
use std::fs;
#[cfg(windows)]
use std::io::Write;
use std::path::Path;

#[cfg(not(windows))]
pub(super) fn remove_installation(
    installation: &InstallationPaths,
    data_directory: &Path,
) -> Result<(), UninstallError> {
    if installation.remove_alias {
        remove_file_if_present(&installation.alias_path)?;
    }
    remove_file_if_present(&installation.executable_path)?;
    remove_directory_if_present(data_directory)?;
    println!("nan-harness uninstalled successfully.");
    Ok(())
}

#[cfg(windows)]
pub(super) fn remove_installation(
    installation: &InstallationPaths,
    data_directory: &Path,
) -> Result<(), UninstallError> {
    use std::process::Command;

    let mut helper = tempfile::Builder::new()
        .prefix("nan-uninstall-")
        .suffix(".ps1")
        .tempfile()
        .map_err(UninstallError::CreateHelper)?;
    helper
        .write_all(WINDOWS_UNINSTALL_HELPER.as_bytes())
        .and_then(|()| helper.flush())
        .map_err(UninstallError::CreateHelper)?;
    let (helper_file, helper_path) = helper
        .keep()
        .map_err(|error| UninstallError::CreateHelper(error.error))?;
    drop(helper_file);

    let alias_path = if installation.remove_alias {
        installation.alias_path.as_os_str()
    } else {
        std::ffi::OsStr::new("")
    };
    let install_directory = installation.executable_path.parent().ok_or_else(|| {
        UninstallError::UnsafeInstallationPath(installation.executable_path.clone())
    })?;
    let result = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&helper_path)
        .arg("-ParentProcessId")
        .arg(std::process::id().to_string())
        .arg("-ExecutablePath")
        .arg(&installation.executable_path)
        .arg("-AliasPath")
        .arg(alias_path)
        .arg("-DataDirectory")
        .arg(data_directory)
        .arg("-InstallDirectory")
        .arg(install_directory)
        .arg("-RemoveUserPath")
        .arg(installation.user_path_entry_added.to_string())
        .spawn();
    if let Err(source) = result {
        let _ = fs::remove_file(&helper_path);
        return Err(UninstallError::StartHelper(source));
    }
    println!("nan-harness uninstall scheduled; cleanup will finish after this process exits.");
    Ok(())
}

#[cfg(windows)]
const WINDOWS_UNINSTALL_HELPER: &str = r#"param(
    [Parameter(Mandatory = $true)][int]$ParentProcessId,
    [Parameter(Mandatory = $true)][string]$ExecutablePath,
    [Parameter(Mandatory = $true)][AllowEmptyString()][string]$AliasPath,
    [Parameter(Mandatory = $true)][string]$DataDirectory,
    [Parameter(Mandatory = $true)][string]$InstallDirectory,
    [Parameter(Mandatory = $true)][string]$RemoveUserPath
)
$ErrorActionPreference = "Stop"
$scriptPath = $MyInvocation.MyCommand.Path
try {
    Wait-Process -Id $ParentProcessId -ErrorAction SilentlyContinue
    if ($AliasPath -and (Test-Path -LiteralPath $AliasPath)) {
        Remove-Item -LiteralPath $AliasPath -Force
    }
    if (Test-Path -LiteralPath $ExecutablePath) {
        Remove-Item -LiteralPath $ExecutablePath -Force
    }
    if (Test-Path -LiteralPath $DataDirectory) {
        Remove-Item -LiteralPath $DataDirectory -Recurse -Force
    }
    if ($RemoveUserPath -eq "true") {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = @($userPath -split ';' | Where-Object {
            $_ -and $_.TrimEnd('\') -ine $InstallDirectory.TrimEnd('\')
        })
        [Environment]::SetEnvironmentVariable("Path", ($entries -join ';'), "User")
    }
    Write-Host "nan-harness uninstalled successfully."
} catch {
    Write-Error "nan-harness uninstall cleanup failed: $_"
} finally {
    Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue
}
"#;

#[cfg(not(windows))]
fn remove_file_if_present(path: &Path) -> Result<(), UninstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::RemoveFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(not(windows))]
fn remove_directory_if_present(path: &Path) -> Result<(), UninstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::RemoveDataDirectory {
            path: path.to_path_buf(),
            source,
        }),
    }
}
