use super::*;
use crate::commands::persistence::publication::{PUBLICATION_HOOK, RECOVERY_HOOK};
use std::cell::Cell;
use std::rc::Rc;

type Snapshot = BTreeMap<PathBuf, (Vec<u8>, Permissions)>;

fn snapshot(root: &Path) -> Snapshot {
    let mut files = BTreeMap::new();
    if !root.exists() {
        return files;
    }
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files.extend(snapshot(&entry.path()));
        } else {
            files.insert(
                entry.path(),
                (
                    fs::read(entry.path()).unwrap(),
                    entry.metadata().unwrap().permissions(),
                ),
            );
        }
    }
    files
}

fn injected_error() -> PersistenceError {
    PersistenceError::WriteFile {
        path: PathBuf::from("synthetic-publication"),
        source: std::io::Error::other("injected"),
    }
}

struct Hook;
impl Drop for Hook {
    fn drop(&mut self) {
        PUBLICATION_HOOK.with_borrow_mut(|hook| *hook = None);
        RECOVERY_HOOK.with_borrow_mut(|hook| *hook = None);
    }
}
fn hook(callback: impl FnMut(usize) -> Result<(), PersistenceError> + 'static) -> Hook {
    PUBLICATION_HOOK.with_borrow_mut(|hook| *hook = Some(Box::new(callback)));
    Hook
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Configure,
    Refresh,
    RefreshWithEmptyMediaOverride,
    Remove,
}

fn operate(
    manager: &ConfigurationManager,
    harness: HarnessKind,
    operation: Operation,
) -> Result<(), ConfigurationError> {
    if matches!(operation, Operation::Remove) {
        return manager.remove(harness).map(|_| ());
    }
    let models = if matches!(operation, Operation::Configure) {
        test_models()
    } else {
        vec![CodingModelProfile::generic("replacement-model")]
    };
    let media = match operation {
        Operation::Configure => Some(MediaSelection::all()),
        Operation::RefreshWithEmptyMediaOverride => Some(MediaSelection::none()),
        _ => None,
    };
    let config = if matches!(operation, Operation::Configure) {
        test_config()
    } else {
        ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: Some("https://api.nan.test/v1".to_owned()),
                nan_api_key: Some(SecretValue::new("rotated-synthetic-key").unwrap()),
            },
        )
        .unwrap()
    };
    let search =
        matches!(operation, Operation::Configure).then_some(if harness == HarnessKind::Aider {
            WebSearchPolicy::Disabled
        } else {
            WebSearchPolicy::Force
        });
    manager
        .configure_with_media(harness, &config, &models, search, media)
        .map(|_| ())
}

fn seed(root: &Path, harness: HarnessKind, operation: Operation) -> ConfigurationManager {
    let manager = ConfigurationManager::new(&root.join("state"), &root.join("home"));
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("unrelated.txt"), b"user-owned content").unwrap();
    // Existing native credentials/settings and catalog content must survive recovery.
    let existing = match harness {
        HarnessKind::OpenCode => Some((".config/opencode/opencode.json", "{\"theme\":\"user\"}\n")),
        HarnessKind::QwenCode => Some((".qwen/settings.json", "{\"userSetting\":true}\n")),
        HarnessKind::DeepSeekHarness => Some((".dsh/settings.yaml", "user-setting: true\n")),
        HarnessKind::Aider => Some((".aider.conf.yml", "# user settings\n")),
        HarnessKind::Hermes => Some((".hermes/.env", "USER_TOKEN=synthetic-user-value\n")),
        _ => None,
    };
    if let Some((path, contents)) = existing {
        let path = home.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    if !matches!(operation, Operation::Configure) {
        operate(&manager, harness, Operation::Configure).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for path in snapshot(root).keys() {
            fs::set_permissions(path, Permissions::from_mode(0o640)).unwrap();
        }
    }
    manager
}

#[test]
fn every_publication_boundary_restores_all_files_and_allows_retry() {
    for harness in SUPPORTED_HARNESSES {
        for operation in [
            Operation::Configure,
            Operation::Refresh,
            Operation::RefreshWithEmptyMediaOverride,
            Operation::Remove,
        ] {
            let probe = tempdir().unwrap();
            let manager = seed(probe.path(), harness, operation);
            let count = Rc::new(Cell::new(0));
            let counter = count.clone();
            {
                let _hook = hook(move |index| {
                    counter.set(index + 1);
                    Ok(())
                });
                operate(&manager, harness, operation).unwrap();
            }
            assert!(count.get() > 0);
            for boundary in 0..count.get() {
                let root = tempdir().unwrap();
                let manager = seed(root.path(), harness, operation);
                let before = snapshot(root.path());
                {
                    let _hook = hook(move |index| {
                        if index == boundary {
                            Err(injected_error())
                        } else {
                            Ok(())
                        }
                    });
                    let error = operate(&manager, harness, operation).unwrap_err();
                    assert!(
                        !matches!(
                            error,
                            ConfigurationError::Persistence(
                                PersistenceError::RollbackIncomplete { .. }
                            )
                        ),
                        "{harness} {operation:?} {boundary}: {error}"
                    );
                }
                assert_eq!(
                    snapshot(root.path()),
                    before,
                    "{harness} {operation:?} boundary {boundary}"
                );
                operate(&manager, harness, operation).unwrap();
                if matches!(operation, Operation::Remove) {
                    assert!(!manager.is_configured(harness).unwrap());
                } else {
                    assert!(
                        manager.is_active(harness).unwrap(),
                        "{harness} {operation:?} boundary {boundary}: {:?}",
                        manager.inspect(harness)
                    );
                }
            }
        }
    }
}

#[test]
fn preparation_conflict_never_publishes_native_credentials() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::QwenCode, Operation::Configure);
    fs::write(
        root.path().join("home/.qwen/settings.json"),
        b"{invalid catalog",
    )
    .unwrap();
    let before = snapshot(root.path());
    assert!(operate(&manager, HarnessKind::QwenCode, Operation::Configure).is_err());
    assert_eq!(snapshot(root.path()), before);
}

#[test]
fn intervening_edit_is_preserved_and_other_published_files_are_restored() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Refresh);
    let before = snapshot(root.path());
    let native_path = root.path().join("home/.aider.conf.yml");
    let changed_path = native_path.clone();
    let error = {
        let _hook = hook(move |index| {
            if index == 3 {
                fs::write(&changed_path, b"# intervening user edit\n").unwrap();
                return Err(injected_error());
            }
            Ok(())
        });
        operate(&manager, HarnessKind::Aider, Operation::Refresh).unwrap_err()
    };
    let ConfigurationError::Persistence(PersistenceError::RollbackIncomplete {
        failures,
        recovery_files,
        ..
    }) = error
    else {
        panic!("expected incomplete recovery");
    };
    assert_eq!(failures, 1);
    assert!(!recovery_files.is_empty());
    for (path, expected) in &before {
        if path != &native_path {
            assert_eq!(
                (
                    fs::read(path).unwrap(),
                    fs::metadata(path).unwrap().permissions()
                ),
                *expected
            );
        }
    }
    assert_eq!(
        fs::read(&native_path).unwrap(),
        b"# intervening user edit\n"
    );
    for path in &recovery_files {
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    // A retry must fail safely while the user's conflicting edit remains.
    assert!(operate(&manager, HarnessKind::Aider, Operation::Refresh).is_err());
    assert_eq!(
        fs::read(&native_path).unwrap(),
        b"# intervening user edit\n"
    );
    fs::write(&native_path, &before[&native_path].0).unwrap();
    operate(&manager, HarnessKind::Aider, Operation::Refresh).unwrap();
}

#[test]
fn restoration_io_failure_attempts_remaining_files_and_retains_usable_snapshots() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Refresh);
    let before = snapshot(root.path());
    let attempts = Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = attempts.clone();
    let error = {
        let _hook = hook(|index| {
            if index == 4 {
                Err(injected_error())
            } else {
                Ok(())
            }
        });
        RECOVERY_HOOK.with_borrow_mut(|hook| {
            *hook = Some(Box::new(move |index| {
                recorded.borrow_mut().push(index);
                if index == 2 {
                    Err(injected_error())
                } else {
                    Ok(())
                }
            }));
        });
        operate(&manager, HarnessKind::Aider, Operation::Refresh).unwrap_err()
    };
    assert_eq!(*attempts.borrow(), vec![3, 2, 1, 0]);
    let ConfigurationError::Persistence(PersistenceError::RollbackIncomplete {
        failures,
        recovery_files,
        ..
    }) = error
    else {
        panic!("expected rollback failure");
    };
    assert_eq!(failures, 1);
    // Recovery copies identify their target and exact prior bytes, including absence.
    for backup in recovery_files {
        let value: Value = serde_json::from_slice(&fs::read(&backup).unwrap()).unwrap();
        let path: PathBuf = serde_json::from_value(value["path"].clone()).unwrap();
        let original: Option<Vec<u8>> = serde_json::from_value(value["original"].clone()).unwrap();
        assert_eq!(original.as_ref(), before.get(&path).map(|(bytes, _)| bytes));
        if let Some(contents) = original {
            fs::write(&path, contents).unwrap();
            fs::set_permissions(&path, before[&path].1.clone()).unwrap();
        } else if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        fs::remove_file(backup).unwrap();
    }
    assert_eq!(snapshot(root.path()), before);
    operate(&manager, HarnessKind::Aider, Operation::Refresh).unwrap();
}

#[test]
fn unpublished_and_unrelated_user_edits_are_never_rolled_back() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Configure);
    let unrelated = root.path().join("home/unrelated.txt");
    let catalog = root.path().join("home/.aider.model.metadata.json");
    let (unrelated_edit, catalog_edit) = (unrelated.clone(), catalog.clone());
    let before = snapshot(root.path());
    {
        let _hook = hook(move |index| {
            if index == 2 {
                fs::write(&unrelated_edit, b"new unrelated content").unwrap();
                fs::write(&catalog_edit, b"new user catalog").unwrap();
            }
            Ok(())
        });
        assert!(operate(&manager, HarnessKind::Aider, Operation::Configure).is_err());
    }
    assert_eq!(fs::read(&unrelated).unwrap(), b"new unrelated content");
    assert_eq!(fs::read(&catalog).unwrap(), b"new user catalog");
    for (path, (bytes, permissions)) in before {
        if path != unrelated {
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::metadata(path).unwrap().permissions(), permissions);
        }
    }
    assert!(!manager.is_configured(HarnessKind::Aider).unwrap());
}

#[test]
fn removal_refuses_to_overwrite_a_user_file_created_after_deletion() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Remove);
    let metadata = root.path().join("home/.aider.model.metadata.json");
    let edited = metadata.clone();
    let before = snapshot(root.path());
    {
        let _hook = hook(move |index| {
            if index == 3 {
                assert!(!edited.exists());
                fs::write(&edited, b"user replacement").unwrap();
                return Err(injected_error());
            }
            Ok(())
        });
        assert!(matches!(
            operate(&manager, HarnessKind::Aider, Operation::Remove),
            Err(ConfigurationError::Persistence(
                PersistenceError::RollbackIncomplete { failures: 1, .. }
            ))
        ));
    }
    assert_eq!(fs::read(&metadata).unwrap(), b"user replacement");
    for (path, expected) in before {
        if path != metadata {
            assert_eq!(
                (
                    fs::read(&path).unwrap(),
                    fs::metadata(path).unwrap().permissions()
                ),
                expected
            );
        }
    }
}

#[test]
fn unchanged_files_are_not_restored_over_later_user_edits() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Refresh);
    let native = root.path().join("home/.aider.conf.yml");
    let edited = native.clone();
    {
        let _hook = hook(move |index| {
            if index == 4 {
                fs::write(&edited, b"user edit to unchanged file").unwrap();
                return Err(injected_error());
            }
            Ok(())
        });
        let error = operate(&manager, HarnessKind::Aider, Operation::Configure).unwrap_err();
        assert!(!matches!(
            error,
            ConfigurationError::Persistence(PersistenceError::RollbackIncomplete { .. })
        ));
    }
    assert_eq!(fs::read(native).unwrap(), b"user edit to unchanged file");
}

#[test]
fn another_harness_and_both_shared_receipts_survive_failure_and_retry() {
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::QwenCode, Operation::Refresh);
    let before = snapshot(root.path());
    {
        let _hook = hook(|index| {
            if index == 4 {
                Err(injected_error())
            } else {
                Ok(())
            }
        });
        assert!(operate(&manager, HarnessKind::Aider, Operation::Configure).is_err());
    }
    assert_eq!(snapshot(root.path()), before);
    assert!(manager.is_active(HarnessKind::QwenCode).unwrap());
    operate(&manager, HarnessKind::Aider, Operation::Configure).unwrap();
    manager.remove(HarnessKind::Aider).unwrap();
    assert!(manager.is_active(HarnessKind::QwenCode).unwrap());
}

#[test]
fn combined_media_and_search_keep_one_plugin_receipt_and_restore_user_plugins() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), &root.path().join("home"));
    let path = root.path().join("home/.hermes/config.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "plugins:\n  enabled: [user/local]\n";
    fs::write(&path, original).unwrap();
    operate(&manager, HarnessKind::Hermes, Operation::Configure).unwrap();
    assert!(manager.is_active(HarnessKind::Hermes).unwrap());
    operate(&manager, HarnessKind::Hermes, Operation::Refresh).unwrap();
    operate(
        &manager,
        HarnessKind::Hermes,
        Operation::RefreshWithEmptyMediaOverride,
    )
    .unwrap();
    let yaml: YamlValue = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        yaml["plugins"]["enabled"],
        serde_yaml_ng::from_str::<YamlValue>(
            "[user/local, web/nan_harness, image_gen/nan_harness]"
        )
        .unwrap()
    );
    manager.remove(HarnessKind::Hermes).unwrap();
    let yaml: YamlValue = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        yaml,
        serde_yaml_ng::from_str::<YamlValue>(original).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn intervening_permission_change_prevents_restoration() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempdir().unwrap();
    let manager = seed(root.path(), HarnessKind::Aider, Operation::Refresh);
    let path = root.path().join("home/.aider.conf.yml");
    let edited = path.clone();
    {
        let _hook = hook(move |index| {
            if index == 4 {
                fs::set_permissions(&edited, Permissions::from_mode(0o400)).unwrap();
                return Err(injected_error());
            }
            Ok(())
        });
        assert!(matches!(
            operate(&manager, HarnessKind::Aider, Operation::Refresh),
            Err(ConfigurationError::Persistence(
                PersistenceError::RollbackIncomplete { failures: 1, .. }
            ))
        ));
    }
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o400
    );
}
