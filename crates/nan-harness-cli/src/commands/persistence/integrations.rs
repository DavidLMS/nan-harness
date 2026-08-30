use super::*;

impl PersistenceManager {
    pub(crate) fn unpersist_pi(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.pi.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed.path.clone().unwrap_or_else(|| {
            let current = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
            if current.exists() {
                current
            } else {
                self.home_directory.join(LEGACY_PI_EXTENSION_RELATIVE_PATH)
            }
        });
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        if let Some(contents) = original.as_deref()
            && sha256(contents) != managed.sha256
        {
            return Err(PersistenceError::ManagedFileChanged(path));
        }
        if original.is_some() {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        }
        state.pi = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn pi_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.pi else {
            return false;
        };
        let path = managed
            .path
            .unwrap_or_else(|| self.home_directory.join(PI_EXTENSION_RELATIVE_PATH));
        fs::read(path).is_ok_and(|contents| sha256(&contents) == managed.sha256)
    }

    pub(crate) fn unpersist_prime_agent(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.prime_agent.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed
            .path
            .clone()
            .unwrap_or_else(|| self.home_directory.join(PRIME_EXTENSION_RELATIVE_PATH));
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        Self::remove_managed_file(&path, &managed)?;
        state.prime_agent = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn prime_agent_is_active(&self) -> bool {
        self.managed_file_is_active(
            |state| state.prime_agent.as_ref(),
            |managed| {
                managed
                    .path
                    .clone()
                    .unwrap_or_else(|| self.prime_directory.join("extensions/nan-provider.js"))
            },
        )
    }

    fn remove_managed_file(path: &Path, managed: &ManagedFile) -> Result<(), PersistenceError> {
        let Some(contents) = read_optional(path)? else {
            return Ok(());
        };
        if sha256(&contents) != managed.sha256 {
            return Err(PersistenceError::ManagedFileChanged(path.to_path_buf()));
        }
        fs::remove_file(path).map_err(|source| PersistenceError::RemoveFile {
            path: path.to_path_buf(),
            source,
        })
    }

    fn managed_file_is_active(
        &self,
        select: impl FnOnce(&IntegrationState) -> Option<&ManagedFile>,
        path: impl FnOnce(&ManagedFile) -> PathBuf,
    ) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = select(&state) else {
            return false;
        };
        fs::read(path(managed)).is_ok_and(|contents| sha256(&contents) == managed.sha256)
    }

    pub(crate) fn configure_opencode(
        &self,
        models: &[CodingModelProfile],
        provider_base_url: &str,
        search: Option<(&str, &str)>,
    ) -> Result<IntegrationChange, PersistenceError> {
        let provider = opencode_provider(models, provider_base_url);
        let provider_hash = hash_input_value(&provider)?;
        let mut state = self.load_state()?;
        let path = self.opencode_config_path(state.opencode.as_ref())?;
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let created_file = original.is_none();
        let source = original.as_deref().map_or_else(
            || "{}\n".to_owned(),
            |value| String::from_utf8_lossy(value).into_owned(),
        );
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value_or_create()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        let provider_property = root_object.get("provider");
        let created_provider_object = provider_property.is_none();
        let providers = match provider_property {
            Some(property) => property
                .object_value()
                .ok_or_else(|| PersistenceError::ProviderIsNotObject(path.clone()))?,
            None => root_object.object_value_or_set("provider"),
        };

        if let Some(existing) = providers.get("nan") {
            let existing_value = existing
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
            let existing_hash = hash_json_value(&existing_value)?;
            match state.opencode.as_ref() {
                Some(managed) if managed.provider_sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedProviderChanged(path));
                }
                None if existing_hash != provider_hash => {
                    return Err(PersistenceError::UnmanagedProviderConflict(path));
                }
                _ => existing.set_value(provider.clone()),
            }
        } else {
            providers.append("nan", provider);
        }

        let selected_model = configure_opencode_model(
            &root_object,
            &path,
            state
                .opencode
                .as_ref()
                .and_then(|managed| managed.selected_model.as_ref()),
            preferred_persistent_model(models),
        )?;
        let search_mcp = configure_opencode_search(
            &root_object,
            &path,
            state
                .opencode
                .as_ref()
                .and_then(|managed| managed.search_mcp.as_ref()),
            search.map(|(api_key, base_url)| opencode_search_server(api_key, base_url)),
        )?;

        let rendered = root.to_string();
        let changed = original.as_deref() != Some(rendered.as_bytes());
        let backup = None;
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.opencode = Some(ManagedOpenCode {
            provider_sha256: provider_hash,
            file_name: file_name(&path)?,
            created_file: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_file)
                || created_file,
            created_provider_object: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_provider_object)
                || created_provider_object,
            selected_model: Some(selected_model),
            search_mcp,
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

    pub(crate) fn unpersist_opencode(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.opencode.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        validate_opencode_file_name(&managed.file_name)?;
        let path = self
            .home_directory
            .join(OPENCODE_CONFIG_DIRECTORY)
            .join(&managed.file_name);
        let original = read_optional(&path)?;
        let Some(contents) = original.as_deref() else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let original_permissions = permissions(&path)?;
        let source = String::from_utf8_lossy(contents);
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        if let Some(selection) = &managed.selected_model {
            remove_opencode_model(&root_object, &path, selection)?;
        }
        if let Some(search) = &managed.search_mcp {
            remove_opencode_search(&root_object, &path, search)?;
        }
        if let Some(providers) = root_object.object_value("provider")
            && let Some(provider) = providers.get("nan")
        {
            let provider_value = provider
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
            if hash_json_value(&provider_value)? != managed.provider_sha256 {
                return Err(PersistenceError::ManagedProviderChanged(path));
            }
            provider.remove();
            if managed.created_provider_object && providers.properties().is_empty() {
                let Some(provider_object) = root_object.get("provider") else {
                    return Err(PersistenceError::ManagedSectionChanged(path.clone()));
                };
                provider_object.remove();
            }
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
        state.opencode = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn opencode_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.opencode else {
            return false;
        };
        if validate_opencode_file_name(&managed.file_name).is_err() {
            return false;
        }
        let path = self
            .home_directory
            .join(OPENCODE_CONFIG_DIRECTORY)
            .join(&managed.file_name);
        let Ok(source) = fs::read_to_string(&path) else {
            return false;
        };
        let Ok(root) = parse_jsonc(&source, &path) else {
            return false;
        };
        let provider_active = root
            .object_value()
            .and_then(|object| object.object_value("provider"))
            .and_then(|providers| providers.get("nan"))
            .and_then(|provider| provider.to_serde_value())
            .and_then(|provider| hash_json_value(&provider).ok())
            .is_some_and(|hash| hash == managed.provider_sha256);
        provider_active
            && managed.selected_model.as_ref().is_none_or(|selection| {
                opencode_model_is_active(root.object_value().as_ref(), selection)
            })
            && managed.search_mcp.as_ref().is_none_or(|search| {
                opencode_search_is_active(root.object_value().as_ref(), search)
            })
    }

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

    pub(super) fn opencode_config_path(
        &self,
        managed: Option<&ManagedOpenCode>,
    ) -> Result<PathBuf, PersistenceError> {
        let directory = self.home_directory.join(OPENCODE_CONFIG_DIRECTORY);
        if let Some(managed) = managed {
            validate_opencode_file_name(&managed.file_name)?;
            return Ok(directory.join(&managed.file_name));
        }
        let json = directory.join(OPENCODE_JSON);
        let jsonc = directory.join(OPENCODE_JSONC);
        match (json.exists(), jsonc.exists()) {
            (true, true) => Err(PersistenceError::AmbiguousOpenCodeConfig(directory)),
            (_, false) => Ok(json),
            (false, true) => Ok(jsonc),
        }
    }
}

fn preferred_persistent_model(models: &[CodingModelProfile]) -> &str {
    models
        .iter()
        .find(|model| model.id == "qwen3.6")
        .or_else(|| models.first())
        .map_or("qwen3.6", |model| model.id.as_str())
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

fn configure_opencode_model(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedOpenCodeModel>,
    model_id: &str,
) -> Result<ManagedOpenCodeModel, PersistenceError> {
    let desired = CstInputValue::String(format!("nan/{model_id}"));
    let value_sha256 = hash_input_value(&desired)?;
    let previous = if let Some(receipt) = managed {
        let current = root
            .get("model")
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        let value = current
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        if hash_json_value(&value)? != receipt.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        current.set_value(desired);
        receipt.previous.clone()
    } else if let Some(current) = root.get("model") {
        let previous = current
            .to_serde_value()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        current.set_value(desired);
        Some(previous)
    } else {
        root.append("model", desired);
        None
    };
    Ok(ManagedOpenCodeModel {
        value_sha256,
        previous,
    })
}

fn remove_opencode_model(
    root: &CstObject,
    path: &Path,
    managed: &ManagedOpenCodeModel,
) -> Result<(), PersistenceError> {
    let current = root
        .get("model")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = current
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    if let Some(previous) = &managed.previous {
        current.set_value(CstInputValue::String(previous.clone()));
    } else {
        current.remove();
    }
    Ok(())
}

fn opencode_model_is_active(root: Option<&CstObject>, managed: &ManagedOpenCodeModel) -> bool {
    root.and_then(|root| root.get("model"))
        .and_then(|property| property.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}

fn opencode_search_server(api_key: &str, base_url: &str) -> CstInputValue {
    CstInputValue::Object(vec![
        ("type".to_owned(), CstInputValue::String("local".to_owned())),
        (
            "command".to_owned(),
            CstInputValue::Array(vec![
                CstInputValue::String("nan-harness".to_owned()),
                CstInputValue::String("__search-mcp".to_owned()),
                CstInputValue::String("--provider-base-url".to_owned()),
                CstInputValue::String(base_url.to_owned()),
                CstInputValue::String("--token-env".to_owned()),
                CstInputValue::String("NAN_HARNESS_SEARCH_API_KEY".to_owned()),
            ]),
        ),
        (
            "environment".to_owned(),
            CstInputValue::Object(vec![(
                "NAN_HARNESS_SEARCH_API_KEY".to_owned(),
                CstInputValue::String(api_key.to_owned()),
            )]),
        ),
        ("enabled".to_owned(), CstInputValue::Bool(true)),
    ])
}

fn configure_opencode_search(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedOpenCodeSearch>,
    desired: Option<CstInputValue>,
) -> Result<Option<ManagedOpenCodeSearch>, PersistenceError> {
    let Some(desired) = desired else {
        if let Some(managed) = managed {
            remove_opencode_search(root, path, managed)?;
        }
        return Ok(None);
    };
    let value_sha256 = hash_input_value(&desired)?;
    let mcp_property = root.get("mcp");
    let created_mcp_object =
        managed.is_some_and(|managed| managed.created_mcp_object) || mcp_property.is_none();
    let servers = match mcp_property {
        Some(property) => property
            .object_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?,
        None => root.object_value_or_set("mcp"),
    };
    if let Some(existing) = servers.get("nan-search") {
        let existing_value = existing
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        let existing_hash = hash_json_value(&existing_value)?;
        let Some(managed) = managed else {
            return Err(PersistenceError::UnmanagedSectionConflict(
                path.to_path_buf(),
            ));
        };
        if existing_hash != managed.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        existing.set_value(desired);
    } else {
        if managed.is_some() {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        servers.append("nan-search", desired);
    }
    Ok(Some(ManagedOpenCodeSearch {
        value_sha256,
        created_mcp_object,
    }))
}

fn remove_opencode_search(
    root: &CstObject,
    path: &Path,
    managed: &ManagedOpenCodeSearch,
) -> Result<(), PersistenceError> {
    let servers = root
        .object_value("mcp")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let search = servers
        .get("nan-search")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = search
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    search.remove();
    if managed.created_mcp_object && servers.properties().is_empty() {
        let mcp = root
            .get("mcp")
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        mcp.remove();
    }
    Ok(())
}

fn opencode_search_is_active(root: Option<&CstObject>, managed: &ManagedOpenCodeSearch) -> bool {
    root.and_then(|root| root.object_value("mcp"))
        .and_then(|servers| servers.get("nan-search"))
        .and_then(|search| search.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}
