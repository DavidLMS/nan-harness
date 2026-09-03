use super::*;

#[test]
fn persistent_search_policy_preserves_external_search_and_transitions_safely() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    let mcp_path = home.join(".cline/data/settings/mcp_settings.json");
    fs::create_dir_all(mcp_path.parent().expect("MCP path should have a parent"))
        .expect("MCP directory should be created");
    fs::write(
        &mcp_path,
        r#"{"mcpServers":{"brave-search":{"command":"brave-search"}}}"#,
    )
    .expect("external search should be written");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("auto configuration should preserve external search");
    let auto: Value = serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
        .expect("MCP config should parse");
    assert!(auto["mcpServers"].get("brave-search").is_some());
    assert!(auto["mcpServers"].get(SEARCH_MCP_ID).is_none());
    let receipt = manager
        .load_state()
        .expect("state should load")
        .harnesses
        .get(&HarnessKind::Cline.to_string())
        .expect("Cline receipt should exist")
        .clone();
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(!receipt.search_managed);

    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Force),
        )
        .expect("force should add managed search");
    let forced: Value =
        serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
            .expect("MCP config should parse");
    assert!(forced["mcpServers"].get("brave-search").is_some());
    assert!(forced["mcpServers"].get(SEARCH_MCP_ID).is_some());

    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Disabled),
        )
        .expect("disabled policy should remove only managed search");
    let disabled: Value =
        serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
            .expect("MCP config should parse");
    assert!(disabled["mcpServers"].get("brave-search").is_some());
    assert!(disabled["mcpServers"].get(SEARCH_MCP_ID).is_none());
}

#[test]
fn persistent_auto_policy_survives_refresh_without_an_override() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("auto should configure search on a clean home");
    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("refresh should preserve auto search");
    let receipt = manager
        .load_state()
        .expect("state should load")
        .harnesses
        .get(&HarnessKind::Cline.to_string())
        .expect("Cline receipt should exist")
        .clone();
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(receipt.search_managed);
}

#[test]
fn pi_family_search_policy_uses_the_runtime_tool_inventory() {
    for (harness, relative_directory) in [
        (HarnessKind::Pi, ".pi/agent"),
        (HarnessKind::PrimeAgent, ".prime/agent"),
    ] {
        let root = tempdir().expect("temporary directory should be created");
        let home = root.path().join("home");
        let state = root.path().join("state");
        let directory = home.join(relative_directory);
        fs::create_dir_all(&directory).expect("Pi-compatible directory should be created");
        let settings_path = directory.join("settings.json");
        fs::write(&settings_path, br#"{"packages":["npm:pi-web-access"]}"#)
            .expect("package configuration should be written");
        let manager = ConfigurationManager::new(&state, &home);
        let config = test_config();
        let models = test_models();
        let extension_path = directory.join(PI_SEARCH_EXTENSION_FILE);

        manager
            .configure(harness, &config, &models, None)
            .expect("automatic search should install a runtime-aware fallback");
        let automatic = fs::read_to_string(&extension_path)
            .expect("automatic search extension should be readable");
        assert!(automatic.contains("const forceNanSearch = false"));
        assert!(automatic.contains("pi.getAllTools()"));
        assert!(automatic.contains("tool.name === \"web_search\""));
        let settings: Value = serde_json::from_slice(
            &fs::read(&settings_path).expect("package configuration should be readable"),
        )
        .expect("package configuration should remain valid JSON");
        assert_eq!(settings["packages"], json!(["npm:pi-web-access"]));

        manager
            .configure(harness, &config, &models, Some(WebSearchPolicy::Force))
            .expect("forced search should replace a package tool");
        let forced = fs::read_to_string(&extension_path)
            .expect("forced search extension should be readable");
        assert!(forced.contains("const forceNanSearch = true"));

        manager
            .configure(harness, &config, &models, Some(WebSearchPolicy::Disabled))
            .expect("disabled search should remove the managed extension");
        assert!(!extension_path.exists());
        assert!(manager.is_active(harness).expect("status should resolve"));
    }
}

#[test]
fn pi_native_refresh_migrates_the_managed_search_mcp_to_an_extension() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state_directory = root.path().join("state");
    let directory = home.join(".pi/agent");
    fs::create_dir_all(&directory).expect("Pi directory should be created");
    let mcp_path = directory.join("mcp.json");
    fs::write(
        &mcp_path,
        br#"{"mcpServers":{"user-owned":{"command":"user-search"}}}"#,
    )
    .expect("user MCP configuration should be written");
    let manager = ConfigurationManager::new(&state_directory, &home);
    let config = test_config();
    let models = test_models();

    let mut old_plans = pi_family_plans(
        &directory,
        "secret-value",
        "https://api.nan.test/v1",
        &models,
        "qwen3.6",
        ManagedSearchStatus {
            policy: WebSearchPolicy::Auto,
            managed: false,
        },
    );
    old_plans.truncate(3);
    old_plans.push(search_mcp_plan(
        mcp_path.clone(),
        "secret-value",
        "https://api.nan.test/v1",
        true,
    ));
    let prepared = prepare_documents(&old_plans, None).expect("old MCP setup should prepare");
    let documents = prepared
        .iter()
        .map(|document| document.receipt.clone())
        .collect();
    apply_prepared(&prepared).expect("old MCP setup should apply");
    let mut state = ConfigurationState::default();
    state.harnesses.insert(
        HarnessKind::Pi.to_string(),
        HarnessReceipt {
            credential_fingerprint: "old-fingerprint".to_owned(),
            model_ids: models.iter().map(|model| model.id.clone()).collect(),
            search_policy: WebSearchPolicy::Auto,
            search_managed: true,
            documents,
        },
    );
    manager
        .save_state(&state)
        .expect("old receipt should be saved");

    manager
        .configure(HarnessKind::Pi, &config, &models, None)
        .expect("refresh should migrate managed search");

    let mcp: Value = serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
        .expect("MCP config should remain valid JSON");
    assert!(mcp["mcpServers"].get("user-owned").is_some());
    assert!(mcp["mcpServers"].get(SEARCH_MCP_ID).is_none());
    let extension = fs::read_to_string(directory.join(PI_SEARCH_EXTENSION_FILE))
        .expect("runtime-aware extension should be installed");
    assert!(extension.contains("const forceNanSearch = false"));
    assert!(!extension.contains("secret-value"));
}

#[test]
fn reserved_search_collision_is_bypassed_only_when_search_is_disabled() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    let mcp_path = home.join(".cline/data/settings/mcp_settings.json");
    fs::create_dir_all(mcp_path.parent().expect("MCP path should have a parent"))
        .expect("MCP directory should be created");
    fs::write(
        &mcp_path,
        r#"{"mcpServers":{"nan-search":{"command":"third-party"}}}"#,
    )
    .expect("collision should be written");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    assert!(matches!(
        manager.configure(HarnessKind::Cline, &config, &models, None),
        Err(ConfigurationError::SearchPolicy(SearchPolicyError::McpNameCollision(path)))
            if path == mcp_path
    ));
    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Disabled),
        )
        .expect("disabled search should preserve the collision untouched");
    assert!(
        fs::read_to_string(mcp_path)
            .expect("collision should remain readable")
            .contains("third-party")
    );
}

#[test]
fn force_search_rejects_aider_without_writing_configuration() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);

    assert!(matches!(
        manager.configure(
            HarnessKind::Aider,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Force),
        ),
        Err(ConfigurationError::SearchPolicy(
            SearchPolicyError::UnsupportedHarness(HarnessKind::Aider)
        ))
    ));
    assert!(!state.join(STATE_FILE_NAME).exists());
}
