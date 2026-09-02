#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct DesktopPaths {
    pub(super) normal_config: PathBuf,
    pub(super) third_party_config: PathBuf,
    pub(super) meta: PathBuf,
    pub(super) profile: PathBuf,
    pub(super) receipt: PathBuf,
    pub(super) backup_directory: PathBuf,
    pub(super) lock: PathBuf,
}

impl DesktopPaths {
    pub(super) fn from_environment(platform: DesktopPlatform) -> Result<Self, ClaudeDesktopError> {
        let environment = DesktopEnvironment::current();
        Self::from_platform_environment(platform, &environment)
    }

    pub(super) fn from_platform_environment(
        platform: DesktopPlatform,
        environment: &DesktopEnvironment,
    ) -> Result<Self, ClaudeDesktopError> {
        let state_override = environment.nan_config.as_deref();
        match platform {
            DesktopPlatform::Macos => {
                let support = environment
                    .home
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingHome)?
                    .join("Library/Application Support");
                let state =
                    state_override.map_or_else(|| support.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &support.join("Claude"),
                    &support.join("Claude-3p"),
                    &state,
                ))
            }
            DesktopPlatform::Linux => {
                let config = environment
                    .xdg_config
                    .clone()
                    .or_else(|| environment.home.as_deref().map(|home| home.join(".config")));
                let config = config.ok_or(ClaudeDesktopError::MissingHome)?;
                let state =
                    state_override.map_or_else(|| config.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &config.join("Claude"),
                    &config.join("Claude-3p"),
                    &state,
                ))
            }
            DesktopPlatform::Windows => {
                let roaming = environment
                    .app_data
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingPlatformDirectory("APPDATA"))?;
                let local = environment
                    .local_app_data
                    .as_deref()
                    .ok_or(ClaudeDesktopError::MissingPlatformDirectory("LOCALAPPDATA"))?;
                let state =
                    state_override.map_or_else(|| roaming.join("nan-harness"), Path::to_path_buf);
                Ok(Self::new(
                    &roaming.join("Claude"),
                    &local.join("Claude-3p"),
                    &state,
                ))
            }
        }
    }

    pub(super) fn new(normal_root: &Path, third_party_root: &Path, state: &Path) -> Self {
        Self {
            normal_config: normal_root.join("claude_desktop_config.json"),
            third_party_config: third_party_root.join("claude_desktop_config.json"),
            meta: third_party_root.join("configLibrary/_meta.json"),
            profile: third_party_root.join(format!("configLibrary/{PROFILE_ID}.json")),
            receipt: state.join("claude-desktop-receipt.json"),
            backup_directory: state.join("claude-desktop-backup"),
            lock: state.join("claude-desktop.lock"),
        }
    }

    pub(super) fn documents(&self) -> [&Path; 4] {
        [
            &self.normal_config,
            &self.third_party_config,
            &self.meta,
            &self.profile,
        ]
    }
}

#[derive(Debug, Default)]
pub(super) struct DesktopEnvironment {
    pub(super) home: Option<PathBuf>,
    pub(super) app_data: Option<PathBuf>,
    pub(super) local_app_data: Option<PathBuf>,
    pub(super) xdg_config: Option<PathBuf>,
    pub(super) nan_config: Option<PathBuf>,
}

impl DesktopEnvironment {
    pub(super) fn current() -> Self {
        Self {
            home: user_home_directory(),
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            xdg_config: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            nan_config: std::env::var_os("NAN_HARNESS_CONFIG_DIR").map(PathBuf::from),
        }
    }
}

pub(super) fn user_home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
