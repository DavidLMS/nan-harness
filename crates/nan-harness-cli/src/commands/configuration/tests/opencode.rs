use super::*;

fn configure(
    manager: &ConfigurationManager,
    key: &str,
) -> Result<ConfigurationChange, ConfigurationError> {
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://api.nan.test/v1".to_owned()),
            nan_api_key: Some(SecretValue::new(key).unwrap()),
        },
    )
    .unwrap();
    manager.configure(
        HarnessKind::OpenCode,
        &config,
        &test_models(),
        Some(WebSearchPolicy::Disabled),
    )
}

#[test]
fn opencode_shared_key_rotation_and_removal_preserve_unmanaged_credentials() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let auth = root.path().join(".local/share/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    let native =
        br#"{"nan":{"type":"api","key":"user-owned"},"other":{"type":"api","key":"other-owned"}}"#;
    fs::write(&auth, native).unwrap();
    let change = configure(&manager, "initial-synthetic").unwrap();
    let key_path = manager.paths.opencode_key_path();
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "initial-synthetic");
    let provider_path = root.path().join(".config/opencode/opencode.json");
    let provider: Value = serde_json::from_slice(&fs::read(&provider_path).unwrap()).unwrap();
    assert_eq!(
        provider["provider"]["nan"]["options"]["apiKey"],
        format!("{{file:{}}}", key_path.display())
    );
    assert!(
        !fs::read_to_string(&provider_path)
            .unwrap()
            .contains("initial-synthetic")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(key_path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    configure(&manager, "rotated-synthetic").unwrap();
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "rotated-synthetic");
    assert!(manager.is_active(HarnessKind::OpenCode).unwrap());
    manager.remove(HarnessKind::OpenCode).unwrap();
    assert!(change.paths.iter().all(|path| !path.exists()));
    assert_eq!(fs::read(auth).unwrap(), native);
}

#[test]
fn opencode_refuses_canonical_provider_conflicts_and_changed_private_keys() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let path = root.path().join(".config/opencode/opencode.jsonc");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        "{\n // user provider\n \"providers\": {\"nan\": {\"package\": \"user-package\"}}\n}\n";
    fs::write(&path, original).unwrap();
    assert!(configure(&manager, "synthetic").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(!manager.paths.opencode_key_path().exists());
    fs::remove_file(&path).unwrap();
    configure(&manager, "synthetic").unwrap();
    fs::write(manager.paths.opencode_key_path(), "user-modified").unwrap();
    assert!(configure(&manager, "rotated").is_err());
    assert!(manager.remove(HarnessKind::OpenCode).is_err());
    assert_eq!(
        fs::read_to_string(manager.paths.opencode_key_path()).unwrap(),
        "user-modified"
    );
}

#[test]
fn opencode_migrates_only_receipt_owned_legacy_auth_entries() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let auth = root.path().join(".local/share/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    fs::write(&auth, br#"{"other":{"type":"api","key":"untouched"}}"#).unwrap();
    let prepared = prepare_documents(
        &[DocumentPlan::Json(JsonPlan {
            path: auth.clone(),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type":"api","key":"legacy-synthetic"}),
            )],
        })],
        None,
    )
    .unwrap();
    apply_prepared(&prepared).unwrap();
    manager
        .legacy
        .configure_opencode(&test_models(), "https://api.nan.test/v1", false)
        .unwrap();
    let mut state = ConfigurationState::default();
    state.harnesses.insert(
        "opencode".to_owned(),
        HarnessReceipt {
            credential_fingerprint: sha256(b"legacy-synthetic"),
            model_ids: vec![],
            search_policy: WebSearchPolicy::Disabled,
            search_managed: false,
            media: MediaSelection::none(),
            documents: prepared
                .iter()
                .map(|document| document.receipt.clone())
                .collect(),
        },
    );
    manager.save_state(&state).unwrap();
    configure(&manager, "new-synthetic").unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&auth).unwrap()).unwrap();
    assert!(value.get("nan").is_none());
    assert_eq!(value["other"]["key"], "untouched");
    // The retired auth file must no longer be inspected or owned on refresh.
    fs::write(&auth, "user-edited-after-migration").unwrap();
    configure(&manager, "rotated-synthetic").unwrap();
    manager.remove(HarnessKind::OpenCode).unwrap();
    assert_eq!(
        fs::read_to_string(auth).unwrap(),
        "user-edited-after-migration"
    );
}
