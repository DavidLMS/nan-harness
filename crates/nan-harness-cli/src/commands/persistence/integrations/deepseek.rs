use super::super::{
    DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END, IntegrationChange, ManagedBlockFormat,
    PersistenceError, PersistenceManager, RemovalOutcome, deepseek_provider_settings,
    inspect_managed_block, optional_utf8, permissions, prepare_managed_block,
    prepare_managed_block_removal, read_optional,
};
use crate::commands::persistence::ConfigurationHealth;
use crate::commands::persistence::PreparedFileChange;
use nan_harness_core::CodingModelProfile;

impl PersistenceManager {
    #[cfg(test)]
    pub(crate) fn configure_deepseek_harness(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let (files, change) = self.prepare_deepseek_harness(models, provider_base_url)?;
        self.publish_configuration_files(&files)?;
        Ok(change)
    }

    pub(crate) fn prepare_deepseek_harness(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<(Vec<PreparedFileChange>, IntegrationChange), PersistenceError> {
        let body = deepseek_provider_settings(models, provider_base_url)?;
        let (mut state, receipt) = self.prepare_state()?;
        let path = state.deepseek_harness.as_ref().map_or_else(
            || self.deepseek_directory.join("settings.yaml"),
            |managed| managed.path.clone(),
        );
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let source = optional_utf8(&path, original.as_deref())?;
        let (rendered, managed) = prepare_managed_block(
            &source,
            &path,
            &body,
            state.deepseek_harness.as_ref(),
            original.is_none(),
            ManagedBlockFormat {
                begin: DEEPSEEK_BLOCK_BEGIN,
                end: DEEPSEEK_BLOCK_END,
                conflicting_keys: &["agent-default-model:", "llm-pi-ai:"],
            },
        )?;
        let changed = source != rendered;
        let backup = None;
        state.deepseek_harness = Some(managed);
        let files = Self::prepare_integration_files(
            vec![PreparedFileChange {
                path: path.clone(),
                original,
                replacement_permissions: original_permissions.clone(),
                original_permissions,
                replacement: Some(rendered.into_bytes()),
            }],
            &state,
            receipt,
        )?;
        Ok((
            files,
            IntegrationChange {
                path,
                additional_paths: Vec::new(),
                backup,
                changed,
            },
        ))
    }

    pub(crate) fn unpersist_deepseek_harness(&self) -> Result<RemovalOutcome, PersistenceError> {
        let (files, outcome) = self.prepare_remove_deepseek_harness()?;
        self.publish_configuration_files(&files)?;
        Ok(outcome)
    }

    pub(crate) fn prepare_remove_deepseek_harness(
        &self,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        let (mut state, receipt) = self.prepare_state()?;
        let Some(managed) = state.deepseek_harness.clone() else {
            return Ok((Vec::new(), RemovalOutcome::NotConfigured));
        };
        let change =
            prepare_managed_block_removal(&managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)?;
        state.deepseek_harness = None;
        Ok((
            Self::prepare_integration_files(vec![change], &state, receipt)?,
            RemovalOutcome::Removed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn deepseek_harness_is_active(&self) -> bool {
        self.inspect_deepseek_harness()
            .is_ok_and(|health| health.is_some_and(ConfigurationHealth::is_active))
    }

    pub(crate) fn inspect_deepseek_harness(
        &self,
    ) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        let state = self.load_state()?;
        Ok(state.deepseek_harness.as_ref().map(|managed| {
            inspect_managed_block(managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)
        }))
    }
}
