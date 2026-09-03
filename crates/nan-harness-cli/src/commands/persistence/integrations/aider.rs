use super::super::{
    AIDER_BLOCK_BEGIN, AIDER_BLOCK_END, AIDER_METADATA_RELATIVE_PATH, AIDER_SETTINGS_RELATIVE_PATH,
    IntegrationChange, ManagedAider, ManagedBlockFormat, PersistenceError, PersistenceManager,
    RemovalOutcome, aider_model_metadata, aider_model_settings, apply_prepared_file_change,
    managed_block_is_active, managed_json_entries_are_active, optional_utf8, permissions,
    prepare_json_entries, prepare_json_entries_removal, prepare_managed_block,
    prepare_managed_block_removal, read_optional, rollback_file, rollback_prepared_file_change,
    write_private_file,
};
use nan_harness_core::CodingModelProfile;

impl PersistenceManager {
    pub(crate) fn configure_aider(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let settings_body = aider_model_settings(models, provider_base_url)?;
        let metadata_entries = aider_model_metadata(models);
        let mut state = self.load_state()?;
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
        if settings_changed {
            write_private_file(
                &settings_path,
                rendered_settings.as_bytes(),
                settings_permissions.as_ref(),
            )?;
        }
        if metadata_changed
            && let Err(error) = write_private_file(
                &metadata_path,
                rendered_metadata.as_bytes(),
                metadata_permissions.as_ref(),
            )
        {
            rollback_file(
                &settings_path,
                original_settings.as_deref(),
                settings_permissions.as_ref(),
            );
            return Err(error);
        }
        state.aider = Some(ManagedAider {
            settings: managed_settings,
            metadata: managed_metadata,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(
                &settings_path,
                original_settings.as_deref(),
                settings_permissions.as_ref(),
            );
            rollback_file(
                &metadata_path,
                original_metadata.as_deref(),
                metadata_permissions.as_ref(),
            );
            return Err(error);
        }
        Ok(IntegrationChange {
            path: settings_path,
            additional_paths: vec![metadata_path],
            backup: None,
            changed: settings_changed || metadata_changed,
        })
    }

    pub(crate) fn unpersist_aider(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.aider.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let settings_change =
            prepare_managed_block_removal(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)?;
        let metadata_change = prepare_json_entries_removal(&managed.metadata)?;
        apply_prepared_file_change(&settings_change)?;
        if let Err(error) = apply_prepared_file_change(&metadata_change) {
            rollback_prepared_file_change(&settings_change);
            return Err(error);
        }
        state.aider = None;
        if let Err(error) = self.save_state(&state) {
            rollback_prepared_file_change(&settings_change);
            rollback_prepared_file_change(&metadata_change);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn aider_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        state.aider.as_ref().is_some_and(|managed| {
            managed_block_is_active(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)
                && managed_json_entries_are_active(&managed.metadata)
        })
    }
}
