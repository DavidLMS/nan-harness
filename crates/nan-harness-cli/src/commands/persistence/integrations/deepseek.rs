use super::super::{
    DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END, IntegrationChange, ManagedBlockFormat,
    PersistenceError, PersistenceManager, RemovalOutcome, apply_prepared_file_change,
    deepseek_provider_settings, managed_block_is_active, optional_utf8, permissions,
    prepare_managed_block, prepare_managed_block_removal, read_optional, rollback_file,
    rollback_prepared_file_change, write_private_file,
};
use nan_harness_core::CodingModelProfile;

impl PersistenceManager {
    pub(crate) fn configure_deepseek_harness(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let body = deepseek_provider_settings(models, provider_base_url)?;
        let mut state = self.load_state()?;
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
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.deepseek_harness = Some(managed);
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            additional_paths: Vec::new(),
            backup,
            changed,
        })
    }

    pub(crate) fn unpersist_deepseek_harness(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.deepseek_harness.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let change =
            prepare_managed_block_removal(&managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)?;
        apply_prepared_file_change(&change)?;
        state.deepseek_harness = None;
        if let Err(error) = self.save_state(&state) {
            rollback_prepared_file_change(&change);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn deepseek_harness_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        state.deepseek_harness.as_ref().is_some_and(|managed| {
            managed_block_is_active(managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)
        })
    }
}
