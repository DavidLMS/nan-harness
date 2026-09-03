use super::super::{
    ManagedQwenAuthSelection, ManagedQwenListDirectory, ManagedQwenModelSelection,
    PersistenceError, hash_input_value, hash_json_value,
};
use jsonc_parser::cst::{CstInputValue, CstObject};
use std::path::Path;

pub(in super::super) fn ensure_qwen_auth_selection(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedQwenAuthSelection>,
) -> Result<Option<ManagedQwenAuthSelection>, PersistenceError> {
    if let Some(managed) = managed {
        let selected = root
            .object_value("security")
            .and_then(|security| security.object_value("auth"))
            .and_then(|auth| auth.get("selectedType"))
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        let value = selected
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        if hash_json_value(&value)? != managed.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        return Ok(Some(managed.clone()));
    }

    let security_property = root.get("security");
    let created_security_object = security_property.is_none();
    let security = match security_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "security",
                    path: path.to_path_buf(),
                })?
        }
        None => root.object_value_or_set("security"),
    };
    let auth_property = security.get("auth");
    let created_auth_object = auth_property.is_none();
    let auth = match auth_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "security.auth",
                    path: path.to_path_buf(),
                })?
        }
        None => security.object_value_or_set("auth"),
    };
    let value = CstInputValue::String("openai".to_owned());
    let value_sha256 = hash_input_value(&value)?;
    let previous = if let Some(selected) = auth.get("selectedType") {
        let previous = selected
            .to_serde_value()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        selected.set_value(value);
        Some(previous)
    } else {
        auth.append("selectedType", value);
        None
    };
    Ok(Some(ManagedQwenAuthSelection {
        value_sha256,
        created_security_object,
        created_auth_object,
        previous,
    }))
}

pub(in super::super) fn remove_qwen_auth_selection(
    root: &CstObject,
    path: &Path,
    managed: &ManagedQwenAuthSelection,
) -> Result<(), PersistenceError> {
    let security = root
        .object_value("security")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let auth = security
        .object_value("auth")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let selected = auth
        .get("selectedType")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = selected
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    if let Some(previous) = &managed.previous {
        selected.set_value(CstInputValue::String(previous.clone()));
    } else {
        selected.remove();
    }
    if managed.created_auth_object && auth.properties().is_empty() {
        let Some(auth_object) = security.get("auth") else {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        };
        auth_object.remove();
    }
    if managed.created_security_object && security.properties().is_empty() {
        let Some(security_object) = root.get("security") else {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        };
        security_object.remove();
    }
    Ok(())
}

pub(in super::super) fn ensure_qwen_model_selection(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedQwenModelSelection>,
    model_id: &str,
) -> Result<ManagedQwenModelSelection, PersistenceError> {
    let model_property = root.get("model");
    let created_model_object = managed.map_or(model_property.is_none(), |receipt| {
        receipt.created_model_object
    });
    let model = match model_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "model",
                    path: path.to_path_buf(),
                })?
        }
        None => root.object_value_or_set("model"),
    };
    let desired = CstInputValue::String(model_id.to_owned());
    let value_sha256 = hash_input_value(&desired)?;
    let previous = if let Some(receipt) = managed {
        let current = model
            .get("name")
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        let value = current
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        if hash_json_value(&value)? != receipt.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        current.set_value(desired);
        receipt.previous.clone()
    } else if let Some(current) = model.get("name") {
        let previous = current
            .to_serde_value()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        current.set_value(desired);
        Some(previous)
    } else {
        model.append("name", desired);
        None
    };
    Ok(ManagedQwenModelSelection {
        value_sha256,
        created_model_object,
        previous,
    })
}

pub(in super::super) fn remove_qwen_model_selection(
    root: &CstObject,
    path: &Path,
    managed: &ManagedQwenModelSelection,
) -> Result<(), PersistenceError> {
    let model = root
        .object_value("model")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let selected = model
        .get("name")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = selected
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    if let Some(previous) = &managed.previous {
        selected.set_value(CstInputValue::String(previous.clone()));
    } else {
        selected.remove();
    }
    if managed.created_model_object && model.properties().is_empty() {
        let Some(model_object) = root.get("model") else {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        };
        model_object.remove();
    }
    Ok(())
}

pub(in super::super) fn ensure_qwen_list_directory(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedQwenListDirectory>,
) -> Result<ManagedQwenListDirectory, PersistenceError> {
    let tools_property = root.get("tools");
    let created_tools_object = managed.map_or(tools_property.is_none(), |receipt| {
        receipt.created_tools_object
    });
    let tools = match tools_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "tools",
                    path: path.to_path_buf(),
                })?
        }
        None => root.object_value_or_set("tools"),
    };
    let list_directory_property = tools.get("listDirectory");
    let created_list_directory_object = managed
        .map_or(list_directory_property.is_none(), |receipt| {
            receipt.created_list_directory_object
        });
    let list_directory = match list_directory_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "tools.listDirectory",
                    path: path.to_path_buf(),
                })?
        }
        None => tools.object_value_or_set("listDirectory"),
    };
    let desired = CstInputValue::Bool(true);
    let value_sha256 = hash_input_value(&desired)?;
    let previous = if let Some(receipt) = managed {
        let current = list_directory
            .get("enabled")
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        let value = current
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        if hash_json_value(&value)? != receipt.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        current.set_value(desired);
        receipt.previous
    } else if let Some(current) = list_directory.get("enabled") {
        let previous = current
            .to_serde_value()
            .and_then(|value| value.as_bool())
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        current.set_value(desired);
        Some(previous)
    } else {
        list_directory.append("enabled", desired);
        None
    };
    Ok(ManagedQwenListDirectory {
        value_sha256,
        created_tools_object,
        created_list_directory_object,
        previous,
    })
}

pub(in super::super) fn remove_qwen_list_directory(
    root: &CstObject,
    path: &Path,
    managed: &ManagedQwenListDirectory,
) -> Result<(), PersistenceError> {
    let tools = root
        .object_value("tools")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let list_directory = tools
        .object_value("listDirectory")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let enabled = list_directory
        .get("enabled")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = enabled
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    if let Some(previous) = managed.previous {
        enabled.set_value(CstInputValue::Bool(previous));
    } else {
        enabled.remove();
    }
    if managed.created_list_directory_object && list_directory.properties().is_empty() {
        let Some(list_directory_object) = tools.get("listDirectory") else {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        };
        list_directory_object.remove();
    }
    if managed.created_tools_object && tools.properties().is_empty() {
        let Some(tools_object) = root.get("tools") else {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        };
        tools_object.remove();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_qwen_auth_selection, ensure_qwen_list_directory, ensure_qwen_model_selection,
        remove_qwen_auth_selection, remove_qwen_list_directory, remove_qwen_model_selection,
    };
    use crate::commands::persistence::parse_named_jsonc;
    use std::path::Path;

    #[test]
    fn qwen_selections_round_trip_preserves_jsonc_and_user_settings() {
        let path = Path::new("settings.json");
        let original = concat!(
            "{\n",
            "  // user settings\n",
            "  \"model\": { \"name\": \"user/model\", \"reasoningEffort\": \"high\" },\n",
            "  \"security\": { \"auth\": { \"selectedType\": \"qwen-oauth\" } },\n",
            "  \"tools\": {\n",
            "    \"listDirectory\": { \"enabled\": false },\n",
            "    \"shell\": { \"enableInteractiveShell\": true }\n",
            "  }\n",
            "}\n",
        );
        let root = parse_named_jsonc(original, path, "Qwen Code")
            .expect("Qwen configuration should parse");
        let object = root.object_value().expect("Qwen root should be an object");

        let auth = ensure_qwen_auth_selection(&object, path, None)
            .expect("auth selection should be prepared")
            .expect("auth selection should be managed");
        let model = ensure_qwen_model_selection(&object, path, None, "nan/model")
            .expect("model selection should be prepared");
        let list_directory = ensure_qwen_list_directory(&object, path, None)
            .expect("list directory setting should be prepared");

        remove_qwen_auth_selection(&object, path, &auth)
            .expect("auth selection should be restored");
        remove_qwen_model_selection(&object, path, &model)
            .expect("model selection should be restored");
        remove_qwen_list_directory(&object, path, &list_directory)
            .expect("list directory setting should be restored");

        assert_eq!(root.to_string(), original);
    }
}
