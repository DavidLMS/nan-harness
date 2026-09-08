use super::{DiscoveryError, Installation};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod asar;

pub(super) fn measure(
    kind: DesktopHarnessKind,
    executable: &Path,
) -> Result<Installation, DiscoveryError> {
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"));
    let mut app_version = None;
    if cfg!(target_os = "macos")
        && let Some(bundle) = bundle
    {
        let mut command = Command::new("/usr/bin/plutil");
        command
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist"));
        app_version = parse_version(&command_output(&mut command)?);
    }
    let parent = executable.parent().ok_or(DiscoveryError::Incomplete)?;
    let resources = bundle.map_or_else(
        || parent.join("resources"),
        |root| root.join("Contents/Resources"),
    );
    if app_version.is_none() {
        app_version = package_version(&resources.join("app/package.json"))?;
    }
    if app_version.is_none() {
        app_version = asar::version(&resources.join("app.asar"))?;
    }
    if app_version.is_none() && cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", "(Get-Item -LiteralPath $env:NAN_CHECK_VERSION_PATH -ErrorAction Stop).VersionInfo.ProductVersion"])
            .env("NAN_CHECK_VERSION_PATH", executable);
        app_version = parse_version(&command_output(&mut command)?);
    }
    // Zed's CLI and bundled Codex have documented --version exits. Electron app
    // executables are never invoked for inventory: --version may open a GUI.
    if app_version.is_none() && kind == DesktopHarnessKind::Zed {
        app_version = parse_version(&command_output(Command::new(executable).arg("--version"))?);
    }
    let runtime_version = if kind == DesktopHarnessKind::ChatGpt {
        let runtime = resources.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        match fs::metadata(&runtime) {
            Ok(metadata) if metadata.is_file() => {
                parse_version(&command_output(Command::new(runtime).arg("--version"))?)
            }
            Ok(_) => return Err(DiscoveryError::Incomplete),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(DiscoveryError::Unreadable),
        }
    } else {
        None
    };
    Ok(Installation {
        executable: executable.to_path_buf(),
        app_version,
        runtime_version,
    })
}

fn package_version(path: &Path) -> Result<Option<Version>, DiscoveryError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DiscoveryError::Unreadable),
    };
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| DiscoveryError::Unreadable)?;
    if bytes.len() > 65_536 {
        return Err(DiscoveryError::Unreadable);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| DiscoveryError::Unreadable)?;
    Ok(value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_version))
}

pub(super) fn parse_version(text: &str) -> Option<Version> {
    text.split_whitespace()
        .find_map(|part| Version::parse(part.trim_start_matches('v')).ok())
}

pub(super) fn command_output(command: &mut Command) -> Result<String, DiscoveryError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("NAN_API_KEY")
        .spawn()
        .map_err(|_| DiscoveryError::Unreadable)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DiscoveryError::Unreadable);
            }
        }
    };
    if !status.success() {
        return Err(DiscoveryError::Unreadable);
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(DiscoveryError::Unreadable)?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| DiscoveryError::Unreadable)?;
    if bytes.len() > 65_536 {
        return Err(DiscoveryError::Unreadable);
    }
    String::from_utf8(bytes).map_err(|_| DiscoveryError::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_exact_not_guessed_from_four_component_numbers() {
        assert_eq!(
            parse_version("codex-cli 0.151.0-alpha.7.2"),
            Version::parse("0.151.0-alpha.7.2").ok()
        );
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("unknown"), None);
    }

    #[test]
    fn absent_package_metadata_is_not_an_inventory_failure() {
        let root = tempfile::tempdir().expect("fixture");
        assert_eq!(package_version(&root.path().join("missing.json")), Ok(None));
    }
}
