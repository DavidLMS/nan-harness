use super::super::{
    ManagedBlock, ManagedJsonEntries, ManagedJsonProperty, ManagedQwenAuthSelection,
    ManagedQwenListDirectory, ManagedQwenModelSelection, PersistenceError, PreparedFileChange,
    hash_json_value, rollback_file, sha256, write_private_file,
};
use super::blocks_json::managed_block_range;
use crate::commands::persistence::health::read_managed_jsonc;
use crate::commands::persistence::{ConfigurationHealth, read_managed_document};
use std::fs;
use std::path::Path;

pub(in super::super) fn inspect_managed_block(
    managed: &ManagedBlock,
    begin: &str,
    end: &str,
) -> ConfigurationHealth {
    let contents = match read_managed_document(&managed.path) {
        Ok(contents) => contents,
        Err(health) => return health,
    };
    let Ok(contents) = std::str::from_utf8(&contents) else {
        return ConfigurationHealth::Invalid;
    };
    match managed_block_range(contents, begin, end) {
        Ok(range) => ConfigurationHealth::from_matches(
            range.is_some_and(|range| sha256(contents[range].as_bytes()) == managed.block_sha256),
        ),
        Err(_) => ConfigurationHealth::Invalid,
    }
}

pub(in super::super) fn inspect_managed_json_entries(
    managed: &ManagedJsonEntries,
) -> ConfigurationHealth {
    let object = match read_managed_jsonc(&managed.path) {
        Ok(object) => object,
        Err(health) => return health,
    };
    ConfigurationHealth::from_matches(managed.entries.iter().all(|(name, expected_hash)| {
        object
            .get(name)
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == *expected_hash)
    }))
}

pub(in super::super) fn inspect_managed_json_property(
    managed: &ManagedJsonProperty,
    parent: &str,
    property: &str,
) -> ConfigurationHealth {
    let object = match read_managed_jsonc(&managed.path) {
        Ok(object) => object,
        Err(health) => return health,
    };
    ConfigurationHealth::from_matches(
        object
            .object_value(parent)
            .and_then(|object| object.get(property))
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == managed.value_sha256),
    )
}

pub(in super::super) fn inspect_qwen_auth_selection(
    path: &Path,
    managed: &ManagedQwenAuthSelection,
) -> ConfigurationHealth {
    let object = match read_managed_jsonc(path) {
        Ok(object) => object,
        Err(health) => return health,
    };
    ConfigurationHealth::from_matches(
        object
            .object_value("security")
            .and_then(|security| security.object_value("auth"))
            .and_then(|auth| auth.get("selectedType"))
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == managed.value_sha256),
    )
}

pub(in super::super) fn inspect_qwen_model_selection(
    path: &Path,
    managed: &ManagedQwenModelSelection,
) -> ConfigurationHealth {
    let object = match read_managed_jsonc(path) {
        Ok(object) => object,
        Err(health) => return health,
    };
    ConfigurationHealth::from_matches(
        object
            .object_value("model")
            .and_then(|model| model.get("name"))
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == managed.value_sha256),
    )
}

pub(in super::super) fn inspect_qwen_list_directory(
    path: &Path,
    managed: &ManagedQwenListDirectory,
) -> ConfigurationHealth {
    let object = match read_managed_jsonc(path) {
        Ok(object) => object,
        Err(health) => return health,
    };
    ConfigurationHealth::from_matches(
        object
            .object_value("tools")
            .and_then(|tools| tools.object_value("listDirectory"))
            .and_then(|list_directory| list_directory.get("enabled"))
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == managed.value_sha256),
    )
}

pub(in super::super) fn apply_prepared_file_change(
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

pub(in super::super) fn rollback_prepared_file_change(change: &PreparedFileChange) {
    rollback_file(
        &change.path,
        change.original.as_deref(),
        change.original_permissions.as_ref(),
    );
}

#[cfg(test)]
mod tests {
    use super::{apply_prepared_file_change, inspect_managed_block, rollback_prepared_file_change};
    use crate::commands::persistence::{ManagedBlockFormat, PreparedFileChange};

    #[test]
    fn apply_and_rollback_preserve_contents_permissions_and_activity() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("settings.yaml");
        let original = b"theme: dark\n";
        std::fs::write(&path, original).expect("original configuration should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("original permissions should be set");
        }
        let original_permissions = std::fs::metadata(&path)
            .expect("original metadata should be readable")
            .permissions();
        let (replacement, managed) = super::super::blocks_json::prepare_managed_block(
            std::str::from_utf8(original).expect("original should be UTF-8"),
            &path,
            "provider: nan",
            None,
            false,
            ManagedBlockFormat {
                begin: "# nan-harness:begin test",
                end: "# nan-harness:end test",
                conflicting_keys: &[],
            },
        )
        .expect("managed block should be prepared");
        let change = PreparedFileChange {
            path: path.clone(),
            original: Some(original.to_vec()),
            original_permissions: Some(original_permissions),
            replacement: Some(replacement.into_bytes()),
        };

        apply_prepared_file_change(&change).expect("prepared change should be applied");
        assert!(
            inspect_managed_block(
                &managed,
                "# nan-harness:begin test",
                "# nan-harness:end test"
            )
            .is_active()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            assert_eq!(
                std::fs::metadata(&path)
                    .expect("managed metadata should be readable")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        rollback_prepared_file_change(&change);

        assert_eq!(
            std::fs::read(&path).expect("restored configuration should be readable"),
            original
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            assert_eq!(
                std::fs::metadata(&path)
                    .expect("restored metadata should be readable")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
