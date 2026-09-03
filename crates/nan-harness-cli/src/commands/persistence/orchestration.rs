use super::{
    AIDER_METADATA_RELATIVE_PATH, AIDER_SETTINGS_RELATIVE_PATH, LastSelection, PersistenceError,
    config_directory, home_directory,
};
use nan_harness_core::{DesktopHarnessKind, HarnessKind, ReasoningSelection};
use std::env;
use std::path::{Path, PathBuf};

const PRIME_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "PRIME_AGENT_CODING_AGENT_DIR";
const QWEN_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "QWEN_HOME";
const DEEPSEEK_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "DSH_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IntegrationChange {
    pub(crate) path: PathBuf,
    pub(crate) additional_paths: Vec<PathBuf>,
    pub(crate) backup: Option<PathBuf>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    Removed,
    NotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistentIntegration {
    OpenCode,
    Pi,
    PrimeAgent,
    QwenCode,
    DeepSeekHarness,
    Aider,
}

impl std::fmt::Display for PersistentIntegration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::OpenCode => "OpenCode",
            Self::Pi => "Pi",
            Self::PrimeAgent => "Prime Agent",
            Self::QwenCode => "Qwen Code",
            Self::DeepSeekHarness => "DeepSeek Harness",
            Self::Aider => "Aider",
        })
    }
}

#[derive(Debug)]
pub(crate) struct PersistenceManager {
    pub(super) state_directory: PathBuf,
    pub(super) state_path: PathBuf,
    pub(super) preferences_path: PathBuf,
    pub(super) home_directory: PathBuf,
    pub(super) prime_directory: PathBuf,
    pub(super) qwen_directory: PathBuf,
    pub(super) deepseek_directory: PathBuf,
}

impl PersistenceManager {
    pub(crate) fn from_environment() -> Result<Self, PersistenceError> {
        let state_directory = config_directory().ok_or(PersistenceError::MissingConfigDirectory)?;
        let home_directory = home_directory().ok_or(PersistenceError::MissingHomeDirectory)?;
        let prime_directory = env::var_os(PRIME_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".prime/agent"), PathBuf::from);
        let qwen_directory = env::var_os(QWEN_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".qwen"), PathBuf::from);
        let deepseek_directory = env::var_os(DEEPSEEK_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".dsh"), PathBuf::from);
        Ok(Self::new_with_directories(
            state_directory,
            home_directory,
            prime_directory,
            qwen_directory,
            deepseek_directory,
        ))
    }

    #[cfg(test)]
    pub(super) fn new(
        state_directory: impl Into<PathBuf>,
        home_directory: impl Into<PathBuf>,
    ) -> Self {
        let home_directory = home_directory.into();
        Self::new_with_directories(
            state_directory,
            home_directory.clone(),
            home_directory.join(".prime/agent"),
            home_directory.join(".qwen"),
            home_directory.join(".dsh"),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(
        state_directory: impl Into<PathBuf>,
        home_directory: impl Into<PathBuf>,
    ) -> Self {
        Self::new(state_directory, home_directory)
    }

    pub(super) fn new_with_directories(
        state_directory: impl Into<PathBuf>,
        home_directory: impl Into<PathBuf>,
        prime_directory: impl Into<PathBuf>,
        qwen_directory: impl Into<PathBuf>,
        deepseek_directory: impl Into<PathBuf>,
    ) -> Self {
        let state_directory = state_directory.into();
        let state_path = state_directory.join("integrations.json");
        let preferences_path = state_directory.join("preferences.json");
        Self {
            state_directory,
            state_path,
            preferences_path,
            home_directory: home_directory.into(),
            prime_directory: prime_directory.into(),
            qwen_directory: qwen_directory.into(),
            deepseek_directory: deepseek_directory.into(),
        }
    }

    pub(crate) fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    pub(crate) fn managed_catalog_paths(
        &self,
        integration: PersistentIntegration,
    ) -> Result<Vec<PathBuf>, PersistenceError> {
        let state = self.load_state()?;
        let paths = match integration {
            PersistentIntegration::OpenCode => {
                vec![self.opencode_config_path(state.opencode.as_ref())?]
            }
            PersistentIntegration::QwenCode => vec![state.qwen_code.as_ref().map_or_else(
                || self.qwen_directory.join("settings.json"),
                |managed| managed.path.clone(),
            )],
            PersistentIntegration::DeepSeekHarness => {
                vec![state.deepseek_harness.as_ref().map_or_else(
                    || self.deepseek_directory.join("settings.yaml"),
                    |managed| managed.path.clone(),
                )]
            }
            PersistentIntegration::Aider => state.aider.as_ref().map_or_else(
                || {
                    vec![
                        self.home_directory.join(AIDER_SETTINGS_RELATIVE_PATH),
                        self.home_directory.join(AIDER_METADATA_RELATIVE_PATH),
                    ]
                },
                |managed| vec![managed.settings.path.clone(), managed.metadata.path.clone()],
            ),
            PersistentIntegration::Pi | PersistentIntegration::PrimeAgent => Vec::new(),
        };
        Ok(paths)
    }

    pub(crate) fn configured_integrations(
        &self,
    ) -> Result<Vec<PersistentIntegration>, PersistenceError> {
        let state = self.load_state()?;
        let mut integrations = Vec::new();
        if state.opencode.is_some() {
            integrations.push(PersistentIntegration::OpenCode);
        }
        if state.pi.is_some() {
            integrations.push(PersistentIntegration::Pi);
        }
        if state.prime_agent.is_some() {
            integrations.push(PersistentIntegration::PrimeAgent);
        }
        if state.qwen_code.is_some() {
            integrations.push(PersistentIntegration::QwenCode);
        }
        if state.deepseek_harness.is_some() {
            integrations.push(PersistentIntegration::DeepSeekHarness);
        }
        if state.aider.is_some() {
            integrations.push(PersistentIntegration::Aider);
        }
        Ok(integrations)
    }

    pub(crate) fn unpersist(
        &self,
        integration: PersistentIntegration,
    ) -> Result<RemovalOutcome, PersistenceError> {
        match integration {
            PersistentIntegration::OpenCode => self.unpersist_opencode(),
            PersistentIntegration::Pi => self.unpersist_pi(),
            PersistentIntegration::PrimeAgent => self.unpersist_prime_agent(),
            PersistentIntegration::QwenCode => self.unpersist_qwen_code(),
            PersistentIntegration::DeepSeekHarness => self.unpersist_deepseek_harness(),
            PersistentIntegration::Aider => self.unpersist_aider(),
        }
    }

    pub(crate) fn integration_is_active(&self, integration: PersistentIntegration) -> bool {
        match integration {
            PersistentIntegration::OpenCode => self.opencode_is_active(),
            PersistentIntegration::Pi => self.pi_is_active(),
            PersistentIntegration::PrimeAgent => self.prime_agent_is_active(),
            PersistentIntegration::QwenCode => self.qwen_code_is_active(),
            PersistentIntegration::DeepSeekHarness => self.deepseek_harness_is_active(),
            PersistentIntegration::Aider => self.aider_is_active(),
        }
    }

    #[cfg(test)]
    pub(crate) fn last_codex_model(&self) -> Result<Option<String>, PersistenceError> {
        Ok(self
            .last_selection(HarnessKind::Codex)?
            .map(|selection| selection.model))
    }

    #[cfg(test)]
    pub(crate) fn save_last_codex_model(&self, model: &str) -> Result<(), PersistenceError> {
        self.save_last_selection(HarnessKind::Codex, model, None)
    }

    pub(crate) fn last_selection(
        &self,
        kind: HarnessKind,
    ) -> Result<Option<LastSelection>, PersistenceError> {
        let preferences = self.load_preferences()?;
        let key = kind.to_string();
        if let Some(selection) = preferences.last_selection_by_harness.get(&key) {
            return Ok(Some(selection.clone()));
        }
        if kind == HarnessKind::Codex {
            return Ok(self
                .load_state()?
                .legacy_last_codex_model
                .map(|model| LastSelection {
                    model,
                    reasoning: None,
                }));
        }
        Ok(None)
    }

    pub(crate) fn save_last_selection(
        &self,
        kind: HarnessKind,
        model: &str,
        reasoning: Option<ReasoningSelection>,
    ) -> Result<(), PersistenceError> {
        if model.is_empty() {
            return Ok(());
        }
        let mut preferences = self.load_preferences()?;
        let key = kind.to_string();
        preferences.last_selection_by_harness.insert(
            key,
            LastSelection {
                model: model.to_owned(),
                reasoning,
            },
        );
        self.save_preferences(&preferences)
    }

    pub(crate) fn last_desktop_selection(
        &self,
        kind: DesktopHarnessKind,
    ) -> Result<Option<LastSelection>, PersistenceError> {
        let key = kind.to_string();
        Ok(self
            .load_preferences()?
            .last_selection_by_desktop
            .get(&key)
            .cloned())
    }

    pub(crate) fn save_last_desktop_selection(
        &self,
        kind: DesktopHarnessKind,
        model: &str,
    ) -> Result<(), PersistenceError> {
        if model.is_empty() {
            return Ok(());
        }
        let mut preferences = self.load_preferences()?;
        let key = kind.to_string();
        preferences.last_selection_by_desktop.insert(
            key,
            LastSelection {
                model: model.to_owned(),
                reasoning: None,
            },
        );
        self.save_preferences(&preferences)
    }
}
