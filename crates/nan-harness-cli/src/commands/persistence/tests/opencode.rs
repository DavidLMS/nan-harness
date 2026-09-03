use super::super::{
    ManagedOpenCode, PersistenceManager, PersistentIntegration, RemovalOutcome, discover_models,
    hash_input_value, opencode_provider, parse_jsonc, write_private_file,
};
use nan_harness_core::SecretValue;
use nan_harness_core::coding_models_from_provider_ids;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};

#[test]
fn opencode_merge_preserves_comments_and_removes_only_nan() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let home = root.path().join("home");
    let config = home.join(".config/opencode/opencode.jsonc");
    std::fs::create_dir_all(config.parent().expect("config should have parent"))
        .expect("config directory should exist");
    std::fs::write(
            &config,
            "{\n  // keep this comment\n  \"provider\": {\n    \"custom\": { \"name\": \"Custom\" },\n  },\n}\n",
        )
        .expect("config should be written");
    let manager = PersistenceManager::new(root.path().join("state"), &home);
    let mut state = manager.load_state().expect("state should load");
    let path = manager
        .opencode_config_path(None)
        .expect("config path should resolve");
    let original = std::fs::read(&path).expect("config should be readable");
    let root_node =
        parse_jsonc(&String::from_utf8_lossy(&original), &path).expect("config should parse");
    let root_object = root_node.object_value().expect("root should be object");
    let providers = root_object
        .object_value("provider")
        .expect("providers should exist");
    let models = coding_models_from_provider_ids(["qwen3.6".to_owned(), "mimo-v2.5".to_owned()]);
    let provider = opencode_provider(&models, "https://api.nan.builders/v1");
    let hash = hash_input_value(&provider).expect("provider should hash");
    providers.append("nan", provider);
    let rendered = root_node.to_string();
    write_private_file(&path, rendered.as_bytes(), None).expect("config should update");
    state.opencode = Some(ManagedOpenCode {
        provider_sha256: hash,
        file_name: "opencode.jsonc".to_owned(),
        created_file: false,
        created_provider_object: false,
        selected_model: None,
        search_mcp: None,
    });
    manager.save_state(&state).expect("state should persist");

    let merged = std::fs::read_to_string(&config).expect("config should be readable");
    assert!(merged.contains("// keep this comment"));
    assert!(merged.contains("\"custom\""));
    assert!(merged.contains("\"nan\""));
    assert!(!merged.contains("NAN_API_KEY"));

    assert_eq!(
        manager
            .unpersist_opencode()
            .expect("OpenCode integration should be removed"),
        RemovalOutcome::Removed
    );
    let restored = std::fs::read_to_string(&config).expect("config should remain");
    assert!(restored.contains("// keep this comment"));
    assert!(restored.contains("\"custom\""));
    assert!(!restored.contains("\"nan\""));
}

#[tokio::test]
async fn opencode_persistence_discovers_the_current_credential_catalog() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let root = tempfile::tempdir().expect("temporary root should exist");
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some(provider.base_url().to_owned()),
            nan_api_key: Some(
                SecretValue::new("test-api-key").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve");

    let models = discover_models(&config)
        .await
        .expect("model catalog should be discovered");
    let change = manager
        .configure_opencode(&models, &config.provider_base_url, None)
        .expect("OpenCode integration should persist");
    let persisted =
        std::fs::read_to_string(&change.path).expect("OpenCode configuration should be readable");
    for model in ["qwen3.6", "deepseek-v4-flash", "mimo-v2.5", "gemma4"] {
        assert!(
            persisted.contains(model),
            "missing discovered model {model}"
        );
    }

    assert!(persisted.contains("\"model\": \"nan/qwen3.6\""));
    assert!(!persisted.contains("test-api-key"));
    assert!(manager.integration_is_active(PersistentIntegration::OpenCode));

    let closing_brace = persisted
        .rfind('}')
        .expect("OpenCode configuration should be an object");
    let mut user_modified = persisted;
    user_modified.insert_str(closing_brace, "  // user-owned note\n");
    std::fs::write(&change.path, user_modified)
        .expect("user comment should be added to the configuration");

    assert_eq!(
        manager
            .unpersist_opencode()
            .expect("OpenCode integration should be removed"),
        RemovalOutcome::Removed
    );
    let preserved =
        std::fs::read_to_string(&change.path).expect("a user-modified configuration should remain");
    assert!(preserved.contains("// user-owned note"));
    assert!(!preserved.contains("\"nan\""));
    assert!(!manager.integration_is_active(PersistentIntegration::OpenCode));

    provider.shutdown().await.expect("provider should stop");
}
