use super::{
    ManagedBlock, ManagedBlockFormat, ManagedFileChange, ManagedJsonEntries, ManagedJsonProperty,
    ManagedQwenAuthSelection, PersistenceError, PreparedFileChange,
    empty_jsonc_object_is_disposable, hash_input_value, hash_json_value, parse_named_jsonc,
    permissions, read_optional, rollback_file, sha256, write_private_file,
};
use jsonc_parser::cst::{CstInputValue, CstObject};
use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

pub(super) fn prepare_managed_block(
    source: &str,
    path: &Path,
    body: &str,
    managed: Option<&ManagedBlock>,
    created_file: bool,
    format: ManagedBlockFormat<'_>,
) -> Result<(String, ManagedBlock), PersistenceError> {
    let desired = format!(
        "{}\n{}{}\n",
        format.begin,
        ensure_trailing_newline(body),
        format.end
    );
    let desired_hash = sha256(desired.as_bytes());
    let current = managed_block_range(source, format.begin, format.end)?;
    let (rendered, added_separator) = if let Some(range) = current {
        let Some(managed) = managed else {
            return Err(PersistenceError::UnmanagedSectionConflict(
                path.to_path_buf(),
            ));
        };
        if sha256(source[range.clone()].as_bytes()) != managed.block_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        let mut rendered = source.to_owned();
        rendered.replace_range(range, &desired);
        (rendered, managed.added_separator)
    } else {
        if managed.is_some() {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        if format
            .conflicting_key
            .is_some_and(|key| source.lines().any(|line| line.trim() == key))
        {
            return Err(PersistenceError::UnmanagedSectionConflict(
                path.to_path_buf(),
            ));
        }
        let added_separator = !source.is_empty() && !source.ends_with('\n');
        let mut rendered = source.to_owned();
        if added_separator {
            rendered.push('\n');
        }
        rendered.push_str(&desired);
        (rendered, added_separator)
    };
    Ok((
        rendered,
        ManagedBlock {
            block_sha256: desired_hash,
            path: path.to_path_buf(),
            created_file: managed.is_some_and(|managed| managed.created_file) || created_file,
            added_separator,
        },
    ))
}

pub(super) fn prepare_json_entries(
    source: &str,
    path: &Path,
    desired: &BTreeMap<String, CstInputValue>,
    managed: Option<&ManagedJsonEntries>,
    created_file: bool,
) -> Result<(String, ManagedJsonEntries), PersistenceError> {
    let source = if source.is_empty() { "{}\n" } else { source };
    let root = parse_named_jsonc(source, path, "Aider")?;
    let object =
        root.object_value_or_create()
            .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
                harness: "Aider",
                path: path.to_path_buf(),
            })?;
    if let Some(managed) = managed {
        for (name, expected_hash) in &managed.entries {
            let property = object
                .get(name)
                .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
            let value = property
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
            if hash_json_value(&value)? != *expected_hash {
                return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
            }
            if !desired.contains_key(name) {
                property.remove();
            }
        }
    } else if object.properties().iter().any(|property| {
        property
            .name()
            .and_then(|name| name.decoded_value().ok())
            .is_some_and(|name| name.starts_with("nan/"))
    }) {
        return Err(PersistenceError::UnmanagedSectionConflict(
            path.to_path_buf(),
        ));
    }
    let mut entries = BTreeMap::new();
    for (name, value) in desired {
        if let Some(existing) = object.get(name) {
            existing.set_value(value.clone());
        } else {
            object.append(name, value.clone());
        }
        entries.insert(name.clone(), hash_input_value(value)?);
    }
    Ok((
        root.to_string(),
        ManagedJsonEntries {
            entries,
            path: path.to_path_buf(),
            created_file: managed.is_some_and(|managed| managed.created_file) || created_file,
        },
    ))
}

pub(super) fn prepare_managed_block_removal(
    managed: &ManagedBlock,
    begin: &str,
    end: &str,
) -> Result<PreparedFileChange, PersistenceError> {
    let original = read_optional(&managed.path)?;
    let original_permissions = permissions(&managed.path)?;
    let Some(contents) = original.as_deref() else {
        return Ok(PreparedFileChange {
            path: managed.path.clone(),
            original,
            original_permissions,
            replacement: None,
        });
    };
    let source = optional_utf8(&managed.path, Some(contents))?;
    let range = managed_block_range(&source, begin, end)?
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(managed.path.clone()))?;
    if sha256(source[range.clone()].as_bytes()) != managed.block_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(
            managed.path.clone(),
        ));
    }
    let mut rendered = source;
    let start = if managed.added_separator && range.start > 0 {
        range.start - 1
    } else {
        range.start
    };
    rendered.replace_range(start..range.end, "");
    let replacement = if managed.created_file && rendered.is_empty() {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedFileChange {
        path: managed.path.clone(),
        original,
        original_permissions,
        replacement,
    })
}

pub(super) fn prepare_json_entries_removal(
    managed: &ManagedJsonEntries,
) -> Result<PreparedFileChange, PersistenceError> {
    let original = read_optional(&managed.path)?;
    let original_permissions = permissions(&managed.path)?;
    let Some(contents) = original.as_deref() else {
        return Ok(PreparedFileChange {
            path: managed.path.clone(),
            original,
            original_permissions,
            replacement: None,
        });
    };
    let source = optional_utf8(&managed.path, Some(contents))?;
    let root = parse_named_jsonc(&source, &managed.path, "Aider")?;
    let object = root
        .object_value()
        .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
            harness: "Aider",
            path: managed.path.clone(),
        })?;
    for (name, expected_hash) in &managed.entries {
        let Some(property) = object.get(name) else {
            continue;
        };
        let value = property
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(managed.path.clone()))?;
        if hash_json_value(&value)? != *expected_hash {
            return Err(PersistenceError::ManagedSectionChanged(
                managed.path.clone(),
            ));
        }
        property.remove();
    }
    let rendered = root.to_string();
    let replacement = if managed.created_file
        && object.properties().is_empty()
        && empty_jsonc_object_is_disposable(&rendered)
    {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedFileChange {
        path: managed.path.clone(),
        original,
        original_permissions,
        replacement,
    })
}

pub(super) fn managed_block_is_active(managed: &ManagedBlock, begin: &str, end: &str) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    managed_block_range(&contents, begin, end)
        .ok()
        .flatten()
        .is_some_and(|range| sha256(contents[range].as_bytes()) == managed.block_sha256)
}

pub(super) fn managed_json_entries_are_active(managed: &ManagedJsonEntries) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, &managed.path, "Aider") else {
        return false;
    };
    let Some(object) = root.object_value() else {
        return false;
    };
    managed.entries.iter().all(|(name, expected_hash)| {
        object
            .get(name)
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == *expected_hash)
    })
}

pub(super) fn managed_json_property_is_active(
    managed: &ManagedJsonProperty,
    parent: &str,
    property: &str,
) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, &managed.path, "managed harness") else {
        return false;
    };
    root.object_value()
        .and_then(|object| object.object_value(parent))
        .and_then(|object| object.get(property))
        .and_then(|property| property.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}

pub(super) fn ensure_qwen_auth_selection(
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
    if auth.get("selectedType").is_some() {
        return Ok(None);
    }
    let value = CstInputValue::String("openai".to_owned());
    let value_sha256 = hash_input_value(&value)?;
    auth.append("selectedType", value);
    Ok(Some(ManagedQwenAuthSelection {
        value_sha256,
        created_security_object,
        created_auth_object,
    }))
}

pub(super) fn remove_qwen_auth_selection(
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
    selected.remove();
    if managed.created_auth_object && auth.properties().is_empty() {
        security
            .get("auth")
            .expect("auth was resolved above")
            .remove();
    }
    if managed.created_security_object && security.properties().is_empty() {
        root.get("security")
            .expect("security was resolved above")
            .remove();
    }
    Ok(())
}

pub(super) fn qwen_auth_selection_is_active(
    path: &Path,
    managed: &ManagedQwenAuthSelection,
) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, path, "Qwen Code") else {
        return false;
    };
    root.object_value()
        .and_then(|root| root.object_value("security"))
        .and_then(|security| security.object_value("auth"))
        .and_then(|auth| auth.get("selectedType"))
        .and_then(|property| property.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}

pub(super) fn managed_block_range(
    source: &str,
    begin: &str,
    end: &str,
) -> Result<Option<Range<usize>>, PersistenceError> {
    let begins = source.match_indices(begin).collect::<Vec<_>>();
    let ends = source.match_indices(end).collect::<Vec<_>>();
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, _)], [(end_start, _)]) if start < end_start => {
            let mut end_index = end_start + end.len();
            if source.as_bytes().get(end_index) == Some(&b'\n') {
                end_index += 1;
            }
            Ok(Some(*start..end_index))
        }
        _ => Err(PersistenceError::InvalidManagedBlock),
    }
}

pub(super) fn ensure_trailing_newline(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

pub(super) fn optional_utf8(path: &Path, value: Option<&[u8]>) -> Result<String, PersistenceError> {
    value.map_or_else(
        || Ok(String::new()),
        |contents| {
            String::from_utf8(contents.to_vec()).map_err(|source| PersistenceError::InvalidUtf8 {
                path: path.to_path_buf(),
                source,
            })
        },
    )
}

pub(super) fn apply_prepared_file_change(
    change: &PreparedFileChange,
) -> Result<(), PersistenceError> {
    match change.replacement.as_deref() {
        Some(contents) => {
            write_private_file(&change.path, contents, change.original_permissions.as_ref())
        }
        None if change.path.exists() => {
            fs::remove_file(&change.path).map_err(|source| PersistenceError::RemoveFile {
                path: change.path.clone(),
                source,
            })
        }
        None => Ok(()),
    }
}

pub(super) fn rollback_prepared_file_change(change: &PreparedFileChange) {
    rollback_file(
        &change.path,
        change.original.as_deref(),
        change.original_permissions.as_ref(),
    );
}

pub(super) fn rollback_managed_change(change: &ManagedFileChange, path: &Path) {
    rollback_file(
        path,
        change.original.as_deref(),
        change.original_permissions.as_ref(),
    );
}
