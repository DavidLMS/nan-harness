use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, SecretError, SecretRef, SecretStore, SecretValue,
};
use nan_harness_i18n::DiagnosticText;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod catalogs;
mod pipeline;
mod values;

pub(crate) struct BridgePreparation {
    pub(crate) base_url: String,
    pub(crate) client_base_url: Option<String>,
    pub(crate) chat_url: Option<String>,
    pub(crate) session_token_ref: SecretRef,
    pub(crate) session_token: Arc<SecretValue>,
    pub(crate) claude_available_models: Vec<String>,
    pub(crate) codex_model_catalog: Option<String>,
    pub(crate) web_search_enabled: bool,
}

pub(crate) struct PreparedLaunch {
    arguments: Vec<String>,
    public_environment: BTreeMap<String, String>,
    runtime_secrets: BTreeMap<SecretRef, Arc<SecretValue>>,
    workspace: TemporaryWorkspace,
}

#[derive(Debug, thiserror::Error)]
pub enum PreparedError {
    #[error(transparent)]
    Temporary(#[from] TemporaryError),
    #[error("launch references unknown temporary artifact '{0}'")]
    UnknownArtifact(String),
    #[error("launch contains unresolved placeholder '{0}'")]
    UnresolvedPlaceholder(DiagnosticText),
    #[error("could not materialize the live NaN model catalog: {0}")]
    ModelCatalog(DiagnosticText),
    #[error("NH-PREPARED-ENV-001")]
    InvalidEnvironmentPathList,
}

impl PreparedLaunch {
    pub(crate) fn prepare(
        plan: &LaunchPlan,
        provider_base_url: &str,
        bridge: Option<BridgePreparation>,
        model_catalog: Option<&[CodingModelProfile]>,
        provider_secrets: &SecretStore,
    ) -> Result<Self, PreparedError> {
        pipeline::prepare(
            plan,
            provider_base_url,
            bridge,
            model_catalog,
            provider_secrets,
        )
    }

    pub(crate) fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub(crate) fn public_environment(&self) -> &BTreeMap<String, String> {
        &self.public_environment
    }

    pub(crate) fn with_secret<T>(
        &self,
        provider_secrets: &SecretStore,
        reference: &SecretRef,
        operation: impl FnOnce(&str) -> T,
    ) -> Result<T, SecretError> {
        if let Some(value) = self.runtime_secrets.get(reference) {
            Ok(value.with_secret(operation))
        } else {
            provider_secrets.with_secret(reference, operation)
        }
    }

    pub(crate) fn temporary_root(&self, has_artifacts: bool) -> Option<PathBuf> {
        has_artifacts.then(|| self.workspace.root().to_path_buf())
    }

    pub(crate) fn artifact_path(&self, artifact_id: &str) -> Option<PathBuf> {
        self.workspace.path(artifact_id).map(Path::to_path_buf)
    }
}

pub(crate) use pipeline::requires_model_catalog;

#[cfg(test)]
mod tests;

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for PreparedError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::Temporary(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::UnknownArtifact(field_0) => {
                m::error_prepared_unknown_artifact(locale, &(field_0))
            }
            Self::UnresolvedPlaceholder(field_0) => m::error_prepared_unresolved_placeholder(
                locale,
                &nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale),
            ),
            Self::ModelCatalog(field_0) => m::error_prepared_model_catalog(
                locale,
                &nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale),
            ),
            Self::InvalidEnvironmentPathList => {
                m::error_prepared_invalid_environment_path_list(locale)
            }
        }
    }
}
