use super::super::{
    ManagedBlock, ManagedBlockFormat, ManagedJsonEntries, PersistenceError, PreparedFileChange,
    empty_jsonc_object_is_disposable, hash_input_value, hash_json_value, parse_named_jsonc,
    permissions, read_optional, sha256,
};
use jsonc_parser::cst::CstInputValue;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

pub(in super::super) fn prepare_managed_block(
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
        if format.conflicting_keys.iter().any(|key| {
            source
                .lines()
                .any(|line| line.trim_start().starts_with(key))
        }) {
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

pub(in super::super) fn prepare_json_entries(
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

pub(in super::super) fn prepare_managed_block_removal(
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

pub(in super::super) fn prepare_json_entries_removal(
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

fn ensure_trailing_newline(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

pub(in super::super) fn optional_utf8(
    path: &Path,
    value: Option<&[u8]>,
) -> Result<String, PersistenceError> {
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

#[cfg(test)]
mod tests {
    use super::{prepare_json_entries, prepare_json_entries_removal, prepare_managed_block};
    use crate::commands::persistence::ManagedBlockFormat;
    use jsonc_parser::cst::CstInputValue;
    use std::collections::BTreeMap;

    #[test]
    fn managed_block_round_trip_preserves_user_content_and_separator() {
        const BEGIN: &str = "# nan-harness:begin test";
        const END: &str = "# nan-harness:end test";

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("settings.yaml");
        let original = "# user setting\ntheme: dark";
        let (rendered, managed) = prepare_managed_block(
            original,
            &path,
            "provider: nan",
            None,
            false,
            ManagedBlockFormat {
                begin: BEGIN,
                end: END,
                conflicting_keys: &[],
            },
        )
        .expect("managed block should be prepared");
        std::fs::write(&path, rendered).expect("managed configuration should be written");

        let change = super::prepare_managed_block_removal(&managed, BEGIN, END)
            .expect("managed block removal should be prepared");

        assert_eq!(change.replacement.as_deref(), Some(original.as_bytes()));
    }

    #[test]
    fn managed_json_round_trip_preserves_jsonc_and_user_entries() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("metadata.json");
        let original = concat!(
            "{\n",
            "  // user metadata\n",
            "  \"custom/model\": { \"max_input_tokens\": 4096 },\n",
            "}\n",
        );
        let desired = BTreeMap::from([(
            "nan/model".to_owned(),
            CstInputValue::Object(vec![(
                "max_input_tokens".to_owned(),
                CstInputValue::Number("8192".to_owned()),
            )]),
        )]);
        let (rendered, managed) = prepare_json_entries(original, &path, &desired, None, false)
            .expect("managed JSON entries should be prepared");
        assert!(rendered.contains("// user metadata"));
        std::fs::write(&path, rendered).expect("managed metadata should be written");

        let change = prepare_json_entries_removal(&managed)
            .expect("managed JSON removal should be prepared");

        assert_eq!(change.replacement.as_deref(), Some(original.as_bytes()));
    }
}
