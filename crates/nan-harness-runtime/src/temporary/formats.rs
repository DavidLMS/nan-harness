use super::TemporaryError;
use super::paths::invalid_artifact;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(super) fn parse_yaml_mapping(
    artifact_id: &str,
    content: &str,
) -> Result<serde_yaml_ng::Mapping, TemporaryError> {
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(content)
        .map_err(|_| invalid_artifact(artifact_id, "NH-TEMP-YAML-001"))?
        .as_mapping()
        .cloned()
        .ok_or_else(|| invalid_artifact(artifact_id, "NH-TEMP-YAML-003"))
}

pub(super) fn merge_yaml_mappings(
    base: &mut serde_yaml_ng::Mapping,
    patch: serde_yaml_ng::Mapping,
) {
    for (key, patch_value) in patch {
        match (base.get_mut(&key), patch_value) {
            (
                Some(serde_yaml_ng::Value::Mapping(base_map)),
                serde_yaml_ng::Value::Mapping(patch_map),
            ) => {
                merge_yaml_mappings(base_map, patch_map);
            }
            (
                Some(serde_yaml_ng::Value::Sequence(base_items)),
                serde_yaml_ng::Value::Sequence(patch_items),
            ) => {
                for item in patch_items {
                    if !base_items.contains(&item) {
                        base_items.push(item);
                    }
                }
            }
            (_, patch_value) => {
                base.insert(key, patch_value);
            }
        }
    }
}

pub(super) fn relocate_hook_state_keys(
    config: &mut toml::Table,
    source_path: &Path,
    target_path: &Path,
) {
    let Some(state) = config
        .get_mut("hooks")
        .and_then(toml::Value::as_table_mut)
        .and_then(|hooks| hooks.get_mut("state"))
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    let Some(source_root) = source_path.parent() else {
        return;
    };
    let Some(target_root) = target_path.parent() else {
        return;
    };
    let source_prefix = format!("{}:", source_root.join("hooks.json").display());
    let mut target_prefixes =
        BTreeSet::from([format!("{}:", target_root.join("hooks.json").display())]);
    if let Ok(canonical_target_root) = fs::canonicalize(target_root) {
        target_prefixes.insert(format!(
            "{}:",
            canonical_target_root.join("hooks.json").display()
        ));
    }
    let keys = state.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(suffix) = key.strip_prefix(&source_prefix) else {
            continue;
        };
        if let Some(value) = state.get(&key).cloned() {
            for target_prefix in &target_prefixes {
                state.insert(format!("{target_prefix}{suffix}"), value.clone());
            }
        }
    }
}

pub(super) fn parse_json_object(
    overlay_id: &str,
    label: &str,
    content: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, TemporaryError> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|error| {
        invalid_artifact(
            overlay_id,
            format!("{label} JSON overlay is invalid: {error}"),
        )
    })?;
    value.as_object().cloned().ok_or_else(|| {
        invalid_artifact(
            overlay_id,
            format!("{label} JSON overlay must be an object"),
        )
    })
}

pub(super) fn merge_json_objects(
    target: &mut serde_json::Map<String, serde_json::Value>,
    patch: serde_json::Map<String, serde_json::Value>,
) {
    for (key, patch_value) in patch {
        match (target.get_mut(&key), patch_value) {
            (
                Some(serde_json::Value::Object(target_object)),
                serde_json::Value::Object(patch_object),
            ) => merge_json_objects(target_object, patch_object),
            (_, patch_value) => {
                target.insert(key, patch_value);
            }
        }
    }
}

pub(super) fn parse_toml_table(
    overlay_id: &str,
    label: &str,
    content: &str,
) -> Result<toml::Table, TemporaryError> {
    toml::from_str(content).map_err(|error| {
        invalid_artifact(
            overlay_id,
            format!("{label} TOML overlay is invalid: {error}"),
        )
    })
}

pub(super) fn merge_toml_tables(target: &mut toml::Table, patch: toml::Table) {
    for (key, patch_value) in patch {
        match (target.get_mut(&key), patch_value) {
            (Some(toml::Value::Table(target_table)), toml::Value::Table(patch_table)) => {
                merge_toml_tables(target_table, patch_table);
            }
            (_, patch_value) => {
                target.insert(key, patch_value);
            }
        }
    }
}
