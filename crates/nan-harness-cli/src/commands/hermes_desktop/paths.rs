#[allow(clippy::wildcard_imports)]
use super::*;

impl DesktopPaths {
    pub(super) fn from_environment() -> Result<Self, HermesDesktopError> {
        let state_directory = config_directory()
            .ok_or(HermesDesktopError::MissingStateDirectory)?
            .join("hermes-desktop");
        if !state_directory.is_absolute() {
            return Err(HermesDesktopError::InvalidStateDirectory);
        }
        let user_home = user_home().ok_or(HermesDesktopError::MissingHomeDirectory)?;
        let hermes_home = resolve_hermes_home(&user_home);
        if !hermes_home.is_absolute() {
            return Err(HermesDesktopError::InvalidHermesHome);
        }
        let user_data = desktop_user_data(&user_home);
        let profiles_root = hermes_home.join("profiles");
        let parked_profiles_root = profiles_root.join(PARKED_PROFILES_DIRECTORY);
        Ok(Self {
            lock: state_directory.join("session.lock"),
            ownership_receipt: state_directory.join("ownership.json"),
            session_receipt: state_directory.join("session.json"),
            backup_directory: state_directory.join("session-backups"),
            install_root: hermes_home.join("hermes-agent"),
            managed_profile: profiles_root.join(PROFILE_NAME),
            parked_profile: parked_profiles_root.join(PROFILE_NAME),
            active_profile: user_data.join("active-profile.json"),
            update_marker: hermes_home.join(".hermes-update-in-progress"),
            recovered_profiles_root: parked_profiles_root.join(RECOVERED_PROFILES_DIRECTORY),
            parked_profiles_root,
            profiles_root,
            hermes_home,
            state_directory,
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(root: &Path) -> Self {
        let state_directory = root.join("state");
        let hermes_home = root.join(".hermes");
        let profiles_root = hermes_home.join("profiles");
        let parked_profiles_root = profiles_root.join(PARKED_PROFILES_DIRECTORY);
        Self {
            lock: state_directory.join("session.lock"),
            ownership_receipt: state_directory.join("ownership.json"),
            session_receipt: state_directory.join("session.json"),
            backup_directory: state_directory.join("session-backups"),
            install_root: hermes_home.join("hermes-agent"),
            managed_profile: profiles_root.join(PROFILE_NAME),
            parked_profile: parked_profiles_root.join(PROFILE_NAME),
            active_profile: root.join("user-data/active-profile.json"),
            update_marker: hermes_home.join(".hermes-update-in-progress"),
            recovered_profiles_root: parked_profiles_root.join(RECOVERED_PROFILES_DIRECTORY),
            parked_profiles_root,
            profiles_root,
            hermes_home,
            state_directory,
        }
    }
}

pub(super) fn resolve_hermes_home(user_home: &Path) -> PathBuf {
    if let Some(explicit) = std::env::var_os("HERMES_HOME") {
        return PathBuf::from(explicit);
    }
    #[cfg(windows)]
    {
        return choose_windows_hermes_home(
            user_home,
            windows_user_scoped_hermes_home(),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        );
    }
    #[cfg(not(windows))]
    {
        user_home.join(".hermes")
    }
}

#[cfg(any(windows, test))]
pub(super) fn choose_windows_hermes_home(
    user_home: &Path,
    user_scoped: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
) -> PathBuf {
    if let Some(user_scoped) = user_scoped.filter(|path| !path.as_os_str().is_empty()) {
        return user_scoped;
    }
    let modern = local_app_data
        .unwrap_or_else(|| user_home.join("AppData/Local"))
        .join("hermes");
    let legacy = user_home.join(".hermes");
    if !modern.is_dir() && legacy.is_dir() {
        legacy
    } else {
        modern
    }
}

#[cfg(windows)]
pub(super) fn windows_user_scoped_hermes_home() -> Option<PathBuf> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('HERMES_HOME','User')",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

pub(super) fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub(super) fn desktop_user_data(user_home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        user_home.join("Library/Application Support/Hermes")
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Hermes"))
            .unwrap_or_else(|| user_home.join("AppData/Roaming/Hermes"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| user_home.join(".config"), PathBuf::from)
            .join("Hermes")
    }
}

pub(super) fn packaged_desktop_candidates(install_root: &Path) -> Vec<PathBuf> {
    let release = install_root.join("apps/desktop/release");
    #[cfg(target_os = "macos")]
    {
        let mut candidates = Vec::new();
        if let Ok(entries) = fs::read_dir(&release) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("mac") {
                    candidates.push(entry.path().join("Hermes.app/Contents/MacOS/Hermes"));
                }
            }
        }
        candidates
    }
    #[cfg(windows)]
    {
        ["win-unpacked", "win-ia32-unpacked", "win-arm64-unpacked"]
            .into_iter()
            .map(|directory| release.join(directory).join("Hermes.exe"))
            .collect()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        ["linux-unpacked", "linux-arm64-unpacked"]
            .into_iter()
            .flat_map(|directory| {
                let directory = release.join(directory);
                ["hermes", "Hermes"]
                    .into_iter()
                    .map(move |binary| directory.join(binary))
            })
            .collect()
    }
}

pub(super) struct SessionLock {
    _file: File,
}

impl SessionLock {
    pub(super) fn acquire(paths: &DesktopPaths) -> Result<Self, HermesDesktopError> {
        fs::create_dir_all(&paths.state_directory)
            .map_err(HermesDesktopError::CreateStateDirectory)?;
        restrict_path(&paths.state_directory, PrivatePathKind::Directory)
            .map_err(HermesDesktopError::ProtectStateDirectory)?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&paths.lock)
            .map_err(HermesDesktopError::OpenLock)?;
        nan_harness_private_fs::restrict_file(&mut file)
            .map_err(HermesDesktopError::ProtectLock)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(HermesDesktopError::ConcurrentSession),
            Err(fs::TryLockError::Error(error)) => Err(HermesDesktopError::Lock(error)),
        }
    }
}
