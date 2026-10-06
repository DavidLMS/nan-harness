use super::{ConfigurationError, Path, PathBuf, STATE_FILE_NAME, config_directory, env};

#[derive(Debug)]
pub(crate) struct ConfigurationPaths {
    pub(crate) state_path: PathBuf,
    pub(crate) home_directory: PathBuf,
    pub(crate) prime_directory: PathBuf,
    pub(crate) omp_directory: PathBuf,
    pub(crate) qwen_directory: PathBuf,
    pub(crate) deepseek_directory: PathBuf,
    pub(crate) kimi_directory: PathBuf,
    pub(crate) opencode_auth_path: PathBuf,
    pub(crate) goose_directory: PathBuf,
    pub(crate) mimo_config_directory: PathBuf,
    pub(crate) mimo_auth_path: PathBuf,
    pub(crate) zcode_provider_path: PathBuf,
}

impl ConfigurationPaths {
    pub(crate) fn from_environment() -> Result<Self, ConfigurationError> {
        let state_directory =
            config_directory().ok_or(ConfigurationError::MissingStateDirectory)?;
        let home_directory = home_directory().ok_or(ConfigurationError::MissingHomeDirectory)?;
        let prime_directory = env::var_os("PRIME_AGENT_CODING_AGENT_DIR")
            .map_or_else(|| home_directory.join(".prime/agent"), PathBuf::from);
        let omp_directory = env::var_os("PI_CODING_AGENT_DIR")
            .map_or_else(|| home_directory.join(".omp/agent"), PathBuf::from);
        let qwen_directory =
            env::var_os("QWEN_HOME").map_or_else(|| home_directory.join(".qwen"), PathBuf::from);
        let deepseek_directory =
            env::var_os("DSH_HOME").map_or_else(|| home_directory.join(".dsh"), PathBuf::from);
        let kimi_directory = env::var_os("KIMI_CODE_HOME")
            .map_or_else(|| home_directory.join(".kimi-code"), PathBuf::from);
        let opencode_auth_path = opencode_auth_path(&home_directory);
        let goose_directory = goose_config_directory(&home_directory);
        let (mimo_config_directory, mimo_auth_path) = mimo_paths(&home_directory);
        Ok(Self {
            state_path: state_directory.join(STATE_FILE_NAME),
            prime_directory,
            omp_directory,
            qwen_directory,
            deepseek_directory,
            kimi_directory,
            opencode_auth_path,
            goose_directory,
            mimo_config_directory,
            mimo_auth_path,
            zcode_provider_path: zcode_provider_path(&home_directory),
            home_directory,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(state_directory: &Path, home_directory: &Path) -> Self {
        Self {
            state_path: state_directory.join(STATE_FILE_NAME),
            home_directory: home_directory.to_path_buf(),
            prime_directory: home_directory.join(".prime/agent"),
            omp_directory: home_directory.join(".omp/agent"),
            qwen_directory: home_directory.join(".qwen"),
            deepseek_directory: home_directory.join(".dsh"),
            kimi_directory: home_directory.join(".kimi-code"),
            opencode_auth_path: home_directory.join(".local/share/opencode/auth.json"),
            goose_directory: home_directory.join(".config/goose"),
            mimo_config_directory: home_directory.join(".config/mimocode"),
            mimo_auth_path: home_directory.join(".local/share/mimocode/auth.json"),
            zcode_provider_path: home_directory.join(".zcode/v2/provider_config.json"),
        }
    }
}

fn opencode_auth_path(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("opencode/auth.json")
    }
    #[cfg(not(windows))]
    {
        env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local/share"), PathBuf::from)
            .join("opencode/auth.json")
    }
}

fn goose_config_directory(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Block/goose/config")
    }
    #[cfg(not(windows))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("goose")
    }
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn mimo_paths(home: &Path) -> (PathBuf, PathBuf) {
    if let Some(root) = env::var_os("MIMOCODE_HOME").filter(|root| !root.is_empty()) {
        let root = PathBuf::from(root);
        return (root.join("config"), root.join("data/auth.json"));
    }
    let config = env::var_os("XDG_CONFIG_HOME")
        .map_or_else(|| home.join(".config"), PathBuf::from)
        .join("mimocode");
    let auth = env::var_os("XDG_DATA_HOME")
        .map_or_else(|| home.join(".local/share"), PathBuf::from)
        .join("mimocode/auth.json");
    (config, auth)
}

fn zcode_provider_path(home: &Path) -> PathBuf {
    env::var_os("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                env::var_os("ZCODE_DATA_BASE_DIR")
                    .filter(|value| !value.is_empty())
                    .map_or_else(|| home.to_path_buf(), PathBuf::from)
                    .join(".zcode/v2/provider_config.json")
            },
            PathBuf::from,
        )
}
