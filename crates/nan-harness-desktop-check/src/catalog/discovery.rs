use super::{DiscoveryError, Installation, versions};
use crate::report::Platform;
use nan_harness_core::DesktopHarnessKind;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Inventory all known installation roots before deciding that installation is safe.
/// Symlink aliases are deduplicated; distinct installations are never silently selected.
///
/// # Errors
/// Returns a closed error for ambiguous, incomplete or unreadable inventory.
pub fn discover(kind: DesktopHarnessKind) -> Result<Option<Installation>, DiscoveryError> {
    let platform = Platform::current();
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(DiscoveryError::Unsupported)?;
    let candidates = candidates(kind, platform, &home)?;
    let Some(executable) = select(candidates)? else {
        return Ok(None);
    };
    versions::measure(kind, &executable).map(Some)
}

/// Inspect an explicitly selected or newly unpacked executable without installing it.
///
/// # Errors
/// Returns a closed error when the executable or its metadata cannot be inspected.
pub fn inspect(kind: DesktopHarnessKind, path: &Path) -> Result<Installation, DiscoveryError> {
    let path = if path.extension().is_some_and(|extension| extension == "app") {
        path.join("Contents/MacOS")
            .join(executable_name(kind, Platform::Macos))
    } else {
        path.to_path_buf()
    };
    let executable = select(vec![path])?.ok_or(DiscoveryError::Incomplete)?;
    versions::measure(kind, &executable)
}

fn candidates(
    kind: DesktopHarnessKind,
    platform: Platform,
    home: &Path,
) -> Result<Vec<PathBuf>, DiscoveryError> {
    let name = executable_name(kind, platform);
    let mut paths = Vec::new();
    match platform {
        Platform::Macos => {
            for directory in [PathBuf::from("/Applications"), home.join("Applications")] {
                let bundle = directory.join(format!("{}.app", app_name(kind)));
                add_bundle(&mut paths, &bundle, name)?;
            }
        }
        Platform::Linux => {
            for directory in [
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/local/bin"),
                home.join(".local/bin"),
            ] {
                paths.push(directory.join(name));
            }
            match kind {
                DesktopHarnessKind::ChatGpt => {
                    paths.push(PathBuf::from("/usr/lib/chatgpt/ChatGPT"));
                }
                DesktopHarnessKind::Zed => paths.push(home.join(".local/zed.app/bin/zed")),
                DesktopHarnessKind::Pen => paths.push(PathBuf::from("/opt/Pen/Pen")),
                DesktopHarnessKind::Hermes => paths.push(PathBuf::from("/opt/Hermes/Hermes")),
                DesktopHarnessKind::Claude => {
                    paths.push(PathBuf::from("/usr/lib/claude-desktop/claude-desktop"));
                }
            }
        }
        Platform::Windows => windows_candidates(kind, home, &mut paths)?,
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path).filter(|directory| directory.is_absolute()) {
            paths.push(directory.join(name));
            if platform == Platform::Linux
                && matches!(kind, DesktopHarnessKind::Pen | DesktopHarnessKind::Hermes)
            {
                paths.push(directory.join(app_name(kind)));
            }
            if kind == DesktopHarnessKind::Zed && platform == Platform::Linux {
                paths.push(directory.join("zeditor"));
                paths.push(directory.join("zed-editor"));
            }
        }
    }
    if kind == DesktopHarnessKind::Hermes {
        hermes_candidates(platform, home, &mut paths)?;
    }
    Ok(paths)
}

fn add_bundle(paths: &mut Vec<PathBuf>, bundle: &Path, name: &str) -> Result<(), DiscoveryError> {
    if exists(bundle)? {
        let executable = bundle.join("Contents/MacOS").join(name);
        if !exists(&executable)? {
            return Err(DiscoveryError::Incomplete);
        }
        paths.push(executable);
    }
    Ok(())
}

fn exists(path: &Path) -> Result<bool, DiscoveryError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(DiscoveryError::Unreadable),
    }
}

fn select(candidates: Vec<PathBuf>) -> Result<Option<PathBuf>, DiscoveryError> {
    let mut found = BTreeSet::new();
    for candidate in candidates {
        if !exists(&candidate)? {
            continue;
        }
        let mut canonical = fs::canonicalize(candidate).map_err(|_| DiscoveryError::Unreadable)?;
        // Managed Zed launches require the CLI shim's --foreground/--wait flags.
        // Deduplicate both entry points onto that shim, not the GUI executable.
        if canonical.ends_with("Zed.app/Contents/MacOS/zed") {
            canonical = canonical.with_file_name("cli");
        }
        // The official Windows bundle places the GUI at Zed/Zed.exe and the
        // managed --foreground/--wait CLI at Zed/bin/zed.exe.
        if canonical
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("zed.exe"))
            && let Some(parent) = canonical.parent()
            && parent
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Zed"))
        {
            canonical = fs::canonicalize(parent.join("bin/zed.exe"))
                .map_err(|_| DiscoveryError::Incomplete)?;
        }
        if let Some(target) = chatgpt_launcher_target(&canonical) {
            canonical = fs::canonicalize(target).map_err(|_| DiscoveryError::Unreadable)?;
        }
        if !fs::metadata(&canonical)
            .map_err(|_| DiscoveryError::Unreadable)?
            .is_file()
        {
            return Err(DiscoveryError::Incomplete);
        }
        found.insert(canonical);
    }
    if found.len() > 1 {
        return Err(DiscoveryError::Ambiguous);
    }
    Ok(found.pop_first())
}

// The official Linux package aliases /usr/bin/chatgpt to this wrapper,
// which launches its sibling ChatGPT. Recognize the package layout without
// executing or interpreting wrappers; the caller validates the direct target.
fn chatgpt_launcher_target(canonical: &Path) -> Option<PathBuf> {
    canonical
        .ends_with("lib/chatgpt/codex-launcher")
        .then(|| canonical.with_file_name("ChatGPT"))
}

fn windows_candidates(
    kind: DesktopHarnessKind,
    home: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), DiscoveryError> {
    let local =
        env::var_os("LOCALAPPDATA").map_or_else(|| home.join("AppData/Local"), PathBuf::from);
    let name = executable_name(kind, Platform::Windows);
    for directory in [local.join("Programs"), local.clone()] {
        paths.push(directory.join(app_name(kind)).join(name));
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        paths.push(PathBuf::from(program_files).join(app_name(kind)).join(name));
    }
    if kind == DesktopHarnessKind::Claude {
        for root in [
            local.join("AnthropicClaude"),
            local.join("Programs/Claude"),
            local.join("Programs/Claude Desktop"),
        ] {
            paths.push(root.join("Claude.exe"));
            for entry in directories(&root)? {
                if entry
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("app-"))
                {
                    paths.push(entry.join("Claude.exe"));
                }
            }
        }
    }
    if matches!(
        kind,
        DesktopHarnessKind::ChatGpt | DesktopHarnessKind::Claude
    ) && cfg!(windows)
    {
        let mut command = std::process::Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", "ConvertTo-Json -Compress -InputObject @(Get-AppxPackage -Name $env:NAN_CHECK_PACKAGE_NAME -ErrorAction Stop | Select-Object -ExpandProperty InstallLocation)"])
            .env("NAN_CHECK_PACKAGE_NAME", if kind == DesktopHarnessKind::ChatGpt { "OpenAI.ChatGPT" } else { "Claude" });
        let output = versions::command_output(&mut command)?;
        let roots: Vec<PathBuf> =
            serde_json::from_str(&output).map_err(|_| DiscoveryError::Unreadable)?;
        for root in roots {
            if !root.is_absolute() {
                return Err(DiscoveryError::Unreadable);
            }
            for relative in [
                format!("app/{name}"),
                name.to_owned(),
                format!("{}/{name}", app_name(kind)),
            ] {
                paths.push(root.join(relative));
            }
        }
    }
    Ok(())
}

fn hermes_candidates(
    platform: Platform,
    home: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), DiscoveryError> {
    let root = env::var_os("HERMES_HOME").map_or_else(
        || {
            if platform == Platform::Windows {
                env::var_os("LOCALAPPDATA")
                    .map_or_else(|| home.join("AppData/Local"), PathBuf::from)
                    .join("hermes")
            } else {
                home.join(".hermes")
            }
        },
        PathBuf::from,
    );
    if !root.is_absolute() {
        return Err(DiscoveryError::Unsupported);
    }
    let release = root.join("hermes-agent/apps/desktop/release");
    for directory in directories(&release)? {
        match platform {
            Platform::Macos => add_bundle(paths, &directory.join("Hermes.app"), "Hermes")?,
            Platform::Linux => {
                paths.push(directory.join("hermes"));
                paths.push(directory.join("Hermes"));
            }
            Platform::Windows => paths.push(directory.join("Hermes.exe")),
        }
    }
    Ok(())
}

fn directories(root: &Path) -> Result<Vec<PathBuf>, DiscoveryError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(DiscoveryError::Unreadable),
    };
    let mut directories = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 256 {
            return Err(DiscoveryError::Unreadable);
        }
        let entry = entry.map_err(|_| DiscoveryError::Unreadable)?;
        if entry
            .metadata()
            .map_err(|_| DiscoveryError::Unreadable)?
            .is_dir()
        {
            directories.push(entry.path());
        }
    }
    Ok(directories)
}

pub(crate) const fn app_name(kind: DesktopHarnessKind) -> &'static str {
    match kind {
        DesktopHarnessKind::ChatGpt => "ChatGPT",
        DesktopHarnessKind::Claude => "Claude",
        DesktopHarnessKind::Hermes => "Hermes",
        DesktopHarnessKind::Pen => "Pen",
        DesktopHarnessKind::Zed => "Zed",
    }
}

const fn executable_name(kind: DesktopHarnessKind, platform: Platform) -> &'static str {
    match (kind, platform) {
        (DesktopHarnessKind::ChatGpt, Platform::Windows) => "ChatGPT.exe",
        (DesktopHarnessKind::Claude, Platform::Windows) => "Claude.exe",
        (DesktopHarnessKind::Hermes, Platform::Windows) => "Hermes.exe",
        (DesktopHarnessKind::Pen, Platform::Windows) => "Pen.exe",
        (DesktopHarnessKind::Zed, Platform::Windows) => "zed.exe",
        (DesktopHarnessKind::ChatGpt, Platform::Linux) => "chatgpt",
        (DesktopHarnessKind::Claude, Platform::Linux) => "claude-desktop",
        (DesktopHarnessKind::Hermes, Platform::Linux) => "hermes-desktop",
        (DesktopHarnessKind::Pen, Platform::Linux) => "pen",
        (DesktopHarnessKind::Zed, _) => "zed",
        (kind, Platform::Macos) => app_name(kind),
    }
}

#[cfg(test)]
mod tests;
