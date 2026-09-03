use super::super::{
    CstObject, IntegrationChange, ManagedOpenCode, ManagedOpenCodeModel, ManagedOpenCodeSearch,
    OPENCODE_CONFIG_DIRECTORY, OPENCODE_JSON, OPENCODE_JSONC, PersistenceError, PersistenceManager,
    RemovalOutcome, empty_jsonc_object_is_disposable, file_name, hash_input_value, hash_json_value,
    opencode_provider, parse_jsonc, permissions, read_optional, rollback_file,
    validate_opencode_file_name, write_private_file,
};
use super::model::preferred_persistent_model;
use jsonc_parser::cst::CstInputValue;
use nan_harness_core::CodingModelProfile;
use std::fs;
use std::path::{Path, PathBuf};

impl PersistenceManager {
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

    pub(in crate::commands::persistence) fn opencode_config_path(
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
