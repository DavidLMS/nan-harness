use super::super::{
    AIDER_BLOCK_BEGIN, AIDER_BLOCK_END, AIDER_METADATA_RELATIVE_PATH, AIDER_SETTINGS_RELATIVE_PATH,
    IntegrationChange, ManagedAider, ManagedBlockFormat, PersistenceError, PersistenceManager,
    RemovalOutcome, aider_model_metadata, aider_model_settings, inspect_managed_block,
    inspect_managed_json_entries, optional_utf8, permissions, prepare_json_entries,
    prepare_json_entries_removal, prepare_managed_block, prepare_managed_block_removal,
    read_optional,
};
use crate::commands::persistence::ConfigurationHealth;
use crate::commands::persistence::PreparedFileChange;
use nan_harness_core::CodingModelProfile;

impl PersistenceManager {
    #[cfg(test)]
    pub(crate) fn configure_aider(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let (files, change) = self.prepare_aider(models, provider_base_url)?;
        self.publish_configuration_files(&files)?;
        Ok(change)
    }

    pub(crate) fn prepare_aider(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<(Vec<PreparedFileChange>, IntegrationChange), PersistenceError> {
        let settings_body = aider_model_settings(models, provider_base_url)?;
        let metadata_entries = aider_model_metadata(models);
        let (mut state, receipt) = self.prepare_state()?;
        let settings_path = state.aider.as_ref().map_or_else(
            || self.home_directory.join(AIDER_SETTINGS_RELATIVE_PATH),
            |managed| managed.settings.path.clone(),
        );
        let metadata_path = state.aider.as_ref().map_or_else(
            || self.home_directory.join(AIDER_METADATA_RELATIVE_PATH),
            |managed| managed.metadata.path.clone(),
        );
        let original_settings = read_optional(&settings_path)?;
        let original_metadata = read_optional(&metadata_path)?;
        let settings_permissions = permissions(&settings_path)?;
        let metadata_permissions = permissions(&metadata_path)?;
        let settings_source = optional_utf8(&settings_path, original_settings.as_deref())?;
        let metadata_source = optional_utf8(&metadata_path, original_metadata.as_deref())?;
        let (rendered_settings, managed_settings) = prepare_managed_block(
            &settings_source,
            &settings_path,
            &settings_body,
            state.aider.as_ref().map(|managed| &managed.settings),
            original_settings.is_none(),
            ManagedBlockFormat {
                begin: AIDER_BLOCK_BEGIN,
                end: AIDER_BLOCK_END,
                conflicting_keys: &["name: nan/"],
            },
        )?;
        let (rendered_metadata, managed_metadata) = prepare_json_entries(
            &metadata_source,
            &metadata_path,
            &metadata_entries,
            state.aider.as_ref().map(|managed| &managed.metadata),
            original_metadata.is_none(),
        )?;
        let settings_changed = settings_source != rendered_settings;
        let metadata_changed = metadata_source != rendered_metadata;
        state.aider = Some(ManagedAider {
            settings: managed_settings,
            metadata: managed_metadata,
        });
        let files = Self::prepare_integration_files(
            vec![
                PreparedFileChange {
                    path: settings_path.clone(),
                    original: original_settings,
                    replacement_permissions: settings_permissions.clone(),
                    original_permissions: settings_permissions,
                    replacement: Some(rendered_settings.into_bytes()),
                },
                PreparedFileChange {
                    path: metadata_path.clone(),
                    original: original_metadata,
                    replacement_permissions: metadata_permissions.clone(),
                    original_permissions: metadata_permissions,
                    replacement: Some(rendered_metadata.into_bytes()),
                },
            ],
            &state,
            receipt,
        )?;
        Ok((
            files,
            IntegrationChange {
                path: settings_path,
                additional_paths: vec![metadata_path],
                backup: None,
                changed: settings_changed || metadata_changed,
            },
        ))
    }

    pub(crate) fn unpersist_aider(&self) -> Result<RemovalOutcome, PersistenceError> {
        let (files, outcome) = self.prepare_remove_aider()?;
        self.publish_configuration_files(&files)?;
        Ok(outcome)
    }

    pub(crate) fn prepare_remove_aider(
        &self,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        let (mut state, receipt) = self.prepare_state()?;
        let Some(managed) = state.aider.clone() else {
            return Ok((Vec::new(), RemovalOutcome::NotConfigured));
        };
        let settings_change =
            prepare_managed_block_removal(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)?;
        let metadata_change = prepare_json_entries_removal(&managed.metadata)?;
        state.aider = None;
        Ok((
            Self::prepare_integration_files(
                vec![settings_change, metadata_change],
                &state,
                receipt,
            )?,
            RemovalOutcome::Removed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn aider_is_active(&self) -> bool {
        self.inspect_aider()
            .is_ok_and(|health| health.is_some_and(ConfigurationHealth::is_active))
    }

    pub(crate) fn inspect_aider(&self) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        let state = self.load_state()?;
        Ok(state.aider.as_ref().map(|managed| {
            inspect_managed_block(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)
                .max(inspect_managed_json_entries(&managed.metadata))
        }))
    }
}
