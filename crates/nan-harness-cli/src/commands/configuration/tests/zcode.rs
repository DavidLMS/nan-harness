use super::*;

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn zcode_refresh_and_removal_preserve_independent_provider_changes() {
    let root = tempdir().unwrap();
    let home = root.path().join("home");
    let manager = ConfigurationManager::new(&root.path().join("state"), &home);
    let path = &manager.paths.zcode_provider_path;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = json!({"schemaVersion": 1, "config": {
        "providerConfigRules": {"providerRules": [{"providerId": "other", "providerName": "Mine"}]},
        "modelConfigRules": {"providerModelRules": []},
        "defaultModelSelection": {"providerId": "other", "modelId": "original"}
    }});
    fs::write(path, serde_json::to_vec(&original).unwrap()).unwrap();
    manager
        .configure(
            HarnessKind::ZCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Force),
        )
        .unwrap();
    let mut current = read(path);
    current["config"]["providerConfigRules"]["providerRules"][0]["providerName"] =
        json!("Changed by user");
    current["config"]["providerConfigRules"]["providerRules"]
        .as_array_mut()
        .unwrap()
        .push(json!({"providerId": "later"}));
    fs::write(path, serde_json::to_vec(&current).unwrap()).unwrap();
    assert!(manager.is_active(HarnessKind::ZCode).unwrap());
    let rotated = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://rotated.nan.test/v1".to_owned()),
            nan_api_key: Some(SecretValue::new("rotated-key").unwrap()),
        },
    )
    .unwrap();
    manager
        .configure(
            HarnessKind::ZCode,
            &rotated,
            &[CodingModelProfile::generic("replacement")],
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let current = read(path);
    let nan = current["config"]["providerConfigRules"]["providerRules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["providerId"] == "nan")
        .unwrap();
    assert_eq!(nan["config"]["access"]["apiKey"], "rotated-key");
    assert_eq!(nan["config"]["personalModelIds"], json!(["replacement"]));
    assert_eq!(
        current["config"]["modelConfigRules"]["providerModelRules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        !fs::read_to_string(&manager.paths.state_path)
            .unwrap()
            .contains("rotated-key")
    );
    manager.remove_all().unwrap();
    let mut expected = original;
    expected["config"]["providerConfigRules"]["providerRules"][0]["providerName"] =
        json!("Changed by user");
    expected["config"]["providerConfigRules"]["providerRules"]
        .as_array_mut()
        .unwrap()
        .push(json!({"providerId": "later"}));
    assert_eq!(read(path), expected);
    assert!(!home.join(".zcode/cli/config.json").exists());
}

#[test]
fn zcode_rejects_foreign_nan_entries_duplicate_members_and_future_schemas() {
    for config in [
        json!({"schemaVersion": 2}),
        json!({"schemaVersion": 1, "config": {"providerConfigRules": {"providerRules": [{"providerId": "nan"}]}}}),
        json!({"schemaVersion": 1, "config": {"providerConfigRules": {"providerRules": [{"providerId": "nan"}, {"providerId": "nan"}]}}}),
        json!({"schemaVersion": 1, "config": {"providerConfigRules": {"providerRules": "wrong-type"}}}),
    ] {
        let root = tempdir().unwrap();
        let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
        let path = &manager.paths.zcode_provider_path;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = serde_json::to_vec(&config).unwrap();
        fs::write(path, &original).unwrap();
        assert!(
            manager
                .configure(
                    HarnessKind::ZCode,
                    &test_config(),
                    &test_models(),
                    Some(WebSearchPolicy::Disabled)
                )
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), original);
        assert!(!manager.paths.state_path.exists());
    }
}

#[test]
fn zcode_user_edit_of_owned_rule_blocks_rotation_and_removal() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    manager
        .configure(
            HarnessKind::ZCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let path = &manager.paths.zcode_provider_path;
    let mut current = read(path);
    current["config"]["providerConfigRules"]["providerRules"][0]["providerName"] =
        json!("user edit");
    fs::write(path, serde_json::to_vec(&current).unwrap()).unwrap();
    let before = fs::read(path).unwrap();
    assert_eq!(
        manager.inspect(HarnessKind::ZCode).unwrap(),
        Some(ConfigurationHealth::Changed)
    );
    assert!(
        manager
            .configure(HarnessKind::ZCode, &test_config(), &test_models(), None)
            .is_err()
    );
    assert!(manager.remove_all().is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn zcode_required_manual_rule_container_preserves_user_members() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    manager
        .configure(
            HarnessKind::ZCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let path = &manager.paths.zcode_provider_path;
    let mut document = read(path);
    assert_eq!(
        document["config"]["modelConfigRules"]["manualProviderModelRules"],
        json!([])
    );
    let user_rule = json!({"providerId":"other", "modelId":"mine", "config":{}});
    document["config"]["modelConfigRules"]["manualProviderModelRules"] = json!([user_rule]);
    fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(manager.is_active(HarnessKind::ZCode).unwrap());
    manager
        .configure(
            HarnessKind::ZCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    manager.remove_all().unwrap();
    assert_eq!(
        read(path),
        json!({"config":{"modelConfigRules":{"manualProviderModelRules":[user_rule]}}})
    );
}
