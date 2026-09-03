use super::super::{
    CstObject, IntegrationChange, ManagedJsonProperty, ManagedQwenAuthSelection, ManagedQwenCode,
    ManagedQwenListDirectory, ManagedQwenModelSelection, PersistenceError, PersistenceManager,
    RemovalOutcome, empty_jsonc_object_is_disposable, ensure_qwen_auth_selection,
    ensure_qwen_list_directory, ensure_qwen_model_selection, hash_input_value, hash_json_value,
    managed_json_property_is_active, parse_named_jsonc, permissions, qwen_auth_selection_is_active,
    qwen_code_provider, qwen_list_directory_is_active, qwen_model_selection_is_active,
    read_optional, remove_qwen_auth_selection, remove_qwen_list_directory,
    remove_qwen_model_selection, rollback_file, write_private_file,
};
use super::model::preferred_persistent_model;
use nan_harness_core::CodingModelProfile;
use std::fs;
use std::path::Path;

impl PersistenceManager {
    pub(crate) fn configure_qwen_code(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let provider = qwen_code_provider(models, provider_base_url);
        let value_hash = hash_input_value(&provider)?;
        let mut state = self.load_state()?;
        let path = state.qwen_code.as_ref().map_or_else(
            || self.qwen_directory.join("settings.json"),
            |managed| managed.path.clone(),
        );
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let created_file = original.is_none();
        let source = original.as_deref().map_or_else(
            || "{}\n".to_owned(),
            |value| String::from_utf8_lossy(value).into_owned(),
        );
        let root = parse_named_jsonc(&source, &path, "Qwen Code")?;
        let root_object = root.object_value_or_create().ok_or_else(|| {
            PersistenceError::ConfigRootIsNotObject {
                harness: "Qwen Code",
                path: path.clone(),
            }
        })?;
        let providers_property = root_object.get("modelProviders");
        let created_parent_object = providers_property.is_none();
        let providers =
            match providers_property {
                Some(property) => property.object_value().ok_or_else(|| {
                    PersistenceError::ConfigFieldIsNotObject {
                        harness: "Qwen Code",
                        field: "modelProviders",
                        path: path.clone(),
                    }
                })?,
                None => root_object.object_value_or_set("modelProviders"),
            };
        if let Some(existing) = providers.get("openai") {
            let existing_value = existing
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.clone()))?;
            let existing_hash = hash_json_value(&existing_value)?;
            match state.qwen_code.as_ref() {
                Some(managed) if managed.value_sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedSectionChanged(path));
                }
                None if existing_hash != value_hash => {
                    return Err(PersistenceError::UnmanagedSectionConflict(path));
                }
                _ => existing.set_value(provider),
            }
        } else {
            providers.append("openai", provider);
        }
        let selections = configure_qwen_selections(
            &root_object,
            &path,
            state.qwen_code.as_ref(),
            preferred_persistent_model(models),
        )?;
        let rendered = root.to_string();
        let changed = original.as_deref() != Some(rendered.as_bytes());
        let backup = None;
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.qwen_code = Some(ManagedQwenCode {
            value_sha256: value_hash,
            path: path.clone(),
            created_file: state
                .qwen_code
                .as_ref()
                .is_some_and(|managed| managed.created_file)
                || created_file,
            created_parent_object: state
                .qwen_code
                .as_ref()
                .is_some_and(|managed| managed.created_parent_object)
                || created_parent_object,
            selected_auth_type: selections.auth_type,
            selected_model: Some(selections.model),
            list_directory: Some(selections.list_directory),
        });
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

    pub(crate) fn unpersist_qwen_code(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.qwen_code.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed.path.clone();
        let Some(contents) = read_optional(&path)? else {
            state.qwen_code = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let original_permissions = permissions(&path)?;
        let source = String::from_utf8_lossy(&contents);
        let root = parse_named_jsonc(&source, &path, "Qwen Code")?;
        let root_object =
            root.object_value()
                .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
                    harness: "Qwen Code",
                    path: path.clone(),
                })?;
        if let Some(providers) = root_object.object_value("modelProviders")
            && let Some(provider) = providers.get("openai")
        {
            let value = provider
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.clone()))?;
            if hash_json_value(&value)? != managed.value_sha256 {
                return Err(PersistenceError::ManagedSectionChanged(path));
            }
            provider.remove();
            if managed.created_parent_object && providers.properties().is_empty() {
                let Some(model_providers) = root_object.get("modelProviders") else {
                    return Err(PersistenceError::ManagedSectionChanged(path.clone()));
                };
                model_providers.remove();
            }
        }
        if let Some(auth_selection) = &managed.selected_auth_type {
            remove_qwen_auth_selection(&root_object, &path, auth_selection)?;
        }
        if let Some(model_selection) = &managed.selected_model {
            remove_qwen_model_selection(&root_object, &path, model_selection)?;
        }
        if let Some(list_directory) = &managed.list_directory {
            remove_qwen_list_directory(&root_object, &path, list_directory)?;
        }
        let rendered = root.to_string();
        if managed.created_file
            && root_object.properties().is_empty()
            && empty_jsonc_object_is_disposable(&rendered)
        {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        } else {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.qwen_code = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, Some(&contents), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn qwen_code_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.qwen_code else {
            return false;
        };
        let provider = ManagedJsonProperty {
            value_sha256: managed.value_sha256,
            path: managed.path.clone(),
            created_file: managed.created_file,
            created_parent_object: managed.created_parent_object,
        };
        managed_json_property_is_active(&provider, "modelProviders", "openai")
            && managed
                .selected_auth_type
                .as_ref()
                .is_none_or(|selection| qwen_auth_selection_is_active(&managed.path, selection))
            && managed
                .selected_model
                .as_ref()
                .is_none_or(|selection| qwen_model_selection_is_active(&managed.path, selection))
            && managed
                .list_directory
                .as_ref()
                .is_none_or(|selection| qwen_list_directory_is_active(&managed.path, selection))
    }
}

struct QwenSelections {
    auth_type: Option<ManagedQwenAuthSelection>,
    model: ManagedQwenModelSelection,
    list_directory: ManagedQwenListDirectory,
}

fn configure_qwen_selections(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedQwenCode>,
    model_id: &str,
) -> Result<QwenSelections, PersistenceError> {
    let auth_type = ensure_qwen_auth_selection(
        root,
        path,
        managed.and_then(|receipt| receipt.selected_auth_type.as_ref()),
    )?;
    let model = ensure_qwen_model_selection(
        root,
        path,
        managed.and_then(|receipt| receipt.selected_model.as_ref()),
        model_id,
    )?;
    let list_directory = ensure_qwen_list_directory(
        root,
        path,
        managed.and_then(|receipt| receipt.list_directory.as_ref()),
    )?;
    Ok(QwenSelections {
        auth_type,
        model,
        list_directory,
    })
}
