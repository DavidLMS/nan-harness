use super::super::documents::parse_json_document;
use super::*;

fn read_config(path: &Path) -> Value {
    parse_json_document(&fs::read(path).expect("configuration"), path, true).expect("JSONC")
}

#[test]
fn mimo_refresh_rotates_credentials_and_restores_user_defaults_and_comments() {
    let root = tempdir().expect("temporary directory");
    let home = root.path().join("home");
    let state = root.path().join("state");
    let manager = ConfigurationManager::new(&state, &home);
    let path = home.join(".config/mimocode/mimocode.jsonc");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = r#"{
        // Keep my preferences.
        "theme": "user",
        "enabled_providers": ["other"],
        "disabled_providers": ["nan", "disabled-other"],
        "model": "other/main",
        "small_model": "other/small",
        "model_groups": {"ultra": "other/ultra", "custom": "other/custom"},
        "provider": {"other": {"name": "User provider"}},
        "mcp": {"user-search": {"type": "local", "command": ["user-search"]}},
    }
"#;
    fs::write(&path, original).unwrap();
    let auth = &manager.paths.mimo_auth_path;
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    let original_auth = json!({"other": {"type": "api", "key": "synthetic-user-key"}});
    fs::write(auth, serde_json::to_vec(&original_auth).unwrap()).unwrap();
    let before = read_config(&path);
    manager
        .configure(
            HarnessKind::MimoCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Force),
        )
        .unwrap();
    assert!(manager.is_active(HarnessKind::MimoCode).unwrap());
    let config = read_config(&path);
    assert_eq!(config["enabled_providers"], json!(["other", "nan"]));
    assert_eq!(config["disabled_providers"], json!(["disabled-other"]));
    assert_eq!(config["model"], "nan/qwen3.6");
    assert_eq!(config["small_model"], "nan/qwen3.6");
    assert_eq!(config["vision_model"], "nan/qwen3.6");
    for tier in ["ultra", "standard", "lite"] {
        assert_eq!(config["model_groups"][tier], "nan/qwen3.6");
    }
    assert_eq!(config["model_groups"]["custom"], "other/custom");
    assert!(config["provider"]["nan"]["models"]["future-model"].is_object());
    assert!(!fs::read_to_string(&path).unwrap().contains("secret-value"));
    assert!(
        !fs::read_to_string(state.join(STATE_FILE_NAME))
            .unwrap()
            .contains("secret-value")
    );
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
            HarnessKind::MimoCode,
            &rotated,
            &[CodingModelProfile::generic("replacement")],
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let config = read_config(&path);
    assert_eq!(config["model"], "nan/replacement");
    assert_eq!(
        config["provider"]["nan"]["options"]["baseURL"],
        "https://rotated.nan.test/v1"
    );
    assert!(config["provider"]["nan"]["models"]["future-model"].is_null());
    assert!(config["mcp"]["nan-search"].is_null());
    assert_eq!(read_config(auth)["nan"]["key"], "rotated-key");
    assert!(manager.is_active(HarnessKind::MimoCode).unwrap());
    assert_eq!(
        manager.remove_all().unwrap(),
        vec![(HarnessKind::MimoCode, RemovalOutcome::Removed)]
    );
    assert_eq!(read_config(&path), before);
    assert_eq!(read_config(auth), original_auth);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("// Keep my preferences.")
    );
    assert!(!manager.is_configured(HarnessKind::MimoCode).unwrap());
}

#[test]
fn mimo_conflicts_and_user_edits_prevent_credential_publication() {
    for conflict in ["provider", "auth", "malformed", "duplicate", "edited"] {
        let root = tempdir().unwrap();
        let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
        let path = manager.paths.mimo_config_directory.join("mimocode.jsonc");
        let auth = &manager.paths.mimo_auth_path;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        match conflict {
            "provider" => fs::write(&path, r#"{"provider":{"nan":{"name":"user"}}}"#).unwrap(),
            "auth" => {
                fs::create_dir_all(auth.parent().unwrap()).unwrap();
                fs::write(auth, r#"{"nan":{"type":"api","key":"user-owned-key"}}"#).unwrap();
            }
            "malformed" => fs::write(&path, "{malformed-private-sentinel").unwrap(),
            "duplicate" => fs::write(&path, r#"{"model":"user/one","model":"user/two"}"#).unwrap(),
            "edited" => {
                manager
                    .configure(
                        HarnessKind::MimoCode,
                        &test_config(),
                        &test_models(),
                        Some(WebSearchPolicy::Disabled),
                    )
                    .unwrap();
                let mut config = read_config(&path);
                config["small_model"] = json!("user/edit");
                fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
                assert_eq!(
                    manager.inspect(HarnessKind::MimoCode).unwrap(),
                    Some(ConfigurationHealth::Changed)
                );
            }
            _ => unreachable!(),
        }
        let before_auth = fs::read(auth).ok();
        let before_config = fs::read(&path).ok();
        let before_state = fs::read(&manager.paths.state_path).ok();
        assert!(
            manager
                .configure(
                    HarnessKind::MimoCode,
                    &test_config(),
                    &test_models(),
                    Some(WebSearchPolicy::Disabled)
                )
                .is_err()
        );
        if conflict == "edited" {
            assert!(manager.remove(HarnessKind::MimoCode).is_err());
        }
        assert_eq!(fs::read(auth).ok(), before_auth);
        assert_eq!(fs::read(&path).ok(), before_config);
        assert_eq!(fs::read(&manager.paths.state_path).ok(), before_state);
    }
}

#[test]
fn mimo_uses_the_highest_precedence_existing_global_config() {
    for name in ["config.json", "mimocode.json", "mimocode.jsonc"] {
        let root = tempdir().unwrap();
        let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
        let directory = &manager.paths.mimo_config_directory;
        fs::create_dir_all(directory).unwrap();
        let path = directory.join(name);
        fs::write(&path, "{ /* user comment */ \"theme\": \"user\", }\n").unwrap();
        manager
            .configure(
                HarnessKind::MimoCode,
                &test_config(),
                &test_models(),
                Some(WebSearchPolicy::Disabled),
            )
            .unwrap();
        assert!(read_config(&path)["provider"]["nan"].is_object());
        manager.remove(HarnessKind::MimoCode).unwrap();
        assert_eq!(read_config(&path), json!({"theme": "user"}));
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .contains("/* user comment */")
        );
    }
}

#[test]
fn mimo_removal_preserves_comments_added_after_configuration() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    manager
        .configure(
            HarnessKind::MimoCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let path = manager.paths.mimo_config_directory.join("mimocode.jsonc");
    let configured = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("// My new note.\n{configured}")).unwrap();
    assert!(manager.is_active(HarnessKind::MimoCode).unwrap());
    manager.remove(HarnessKind::MimoCode).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("// My new note.")
    );
    assert_eq!(read_config(&path), json!({}));
}

#[test]
fn invalid_mimo_paths_fail_before_writes_and_leave_other_harnesses_usable() {
    let root = tempdir().unwrap();
    let mut manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    manager.paths.mimo_config_directory = PathBuf::from("relative/config");
    manager
        .configure(
            HarnessKind::Pi,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    assert!(manager.is_active(HarnessKind::Pi).unwrap());
    assert!(
        manager
            .configure(
                HarnessKind::MimoCode,
                &test_config(),
                &test_models(),
                Some(WebSearchPolicy::Disabled)
            )
            .is_err()
    );
    assert!(!manager.paths.mimo_auth_path.exists());
    assert_eq!(
        manager.configured_harnesses().unwrap(),
        vec![HarnessKind::Pi]
    );
}

#[test]
fn mimo_layered_global_configs_preserve_filters_and_reject_lower_nan_credentials() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let directory = &manager.paths.mimo_config_directory;
    fs::create_dir_all(directory).unwrap();
    let lower = directory.join("config.json");
    let higher = directory.join("mimocode.jsonc");
    let original =
        json!({"enabled_providers": ["other"], "disabled_providers": ["nan", "disabled-other"]});
    fs::write(&lower, serde_json::to_vec(&original).unwrap()).unwrap();
    fs::write(&higher, "{ /* higher layer */ \"theme\": \"user\" }\n").unwrap();
    manager
        .configure(
            HarnessKind::MimoCode,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    assert_eq!(
        read_config(&higher)["enabled_providers"],
        json!(["other", "nan"])
    );
    assert_eq!(
        read_config(&higher)["disabled_providers"],
        json!(["disabled-other"])
    );
    assert_eq!(read_config(&lower), original);
    manager.remove(HarnessKind::MimoCode).unwrap();
    assert_eq!(read_config(&higher), json!({"theme": "user"}));
    fs::write(
        &lower,
        r#"{"provider":{"nan":{"options":{"apiKey":"synthetic-user-key"}}}}"#,
    )
    .unwrap();
    assert!(
        manager
            .configure(
                HarnessKind::MimoCode,
                &test_config(),
                &test_models(),
                Some(WebSearchPolicy::Disabled)
            )
            .is_err()
    );
    assert!(!manager.paths.mimo_auth_path.exists());
    assert_eq!(read_config(&higher), json!({"theme": "user"}));
}
