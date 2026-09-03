use super::super::{
    AIDER_METADATA_RELATIVE_PATH, AIDER_SETTINGS_RELATIVE_PATH, PersistenceManager, RemovalOutcome,
    discover_models,
};
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use std::path::Path;

#[tokio::test]
async fn persistent_catalogs_are_dynamic_secret_free_and_reversible() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let root = tempfile::tempdir().expect("temporary root should exist");
    let home = root.path().join("home");
    let qwen = root.path().join("qwen-home");
    let deepseek = root.path().join("deepseek-home");
    let manager = PersistenceManager::new_with_directories(
        root.path().join("state"),
        &home,
        home.join(".prime/agent"),
        &qwen,
        &deepseek,
    );
    let qwen_path = qwen.join("settings.json");
    let deepseek_path = deepseek.join("settings.yaml");
    let aider_settings = home.join(AIDER_SETTINGS_RELATIVE_PATH);
    let aider_metadata = home.join(AIDER_METADATA_RELATIVE_PATH);
    for path in [&qwen_path, &deepseek_path, &aider_settings, &aider_metadata] {
        std::fs::create_dir_all(path.parent().expect("config should have parent"))
            .expect("configuration directory should exist");
    }
    let qwen_original = "{\n  // user setting\n  \"theme\": \"dark\"\n}\n";
    let deepseek_original = "# user setting\nui:\n  theme: dark\n";
    let aider_settings_original = "- name: custom/model\n  edit_format: whole\n";
    let aider_metadata_original = "{\n  \"custom/model\": { \"max_input_tokens\": 4096 }\n}\n";
    std::fs::write(&qwen_path, qwen_original).expect("Qwen config should be written");
    std::fs::write(&deepseek_path, deepseek_original).expect("DeepSeek config should be written");
    std::fs::write(&aider_settings, aider_settings_original)
        .expect("Aider settings should be written");
    std::fs::write(&aider_metadata, aider_metadata_original)
        .expect("Aider metadata should be written");
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
    let qwen_change = manager
        .configure_qwen_code(&models, &config.provider_base_url)
        .expect("Qwen Code integration should persist");
    let deepseek_change = manager
        .configure_deepseek_harness(&models, &config.provider_base_url)
        .expect("DeepSeek integration should persist");
    let aider_change = manager
        .configure_aider(&models, &config.provider_base_url)
        .expect("Aider integration should persist");

    assert_persisted_catalogs(
        [
            &qwen_change.path,
            &deepseek_change.path,
            &aider_change.path,
            &aider_change.additional_paths[0],
        ],
        &qwen_path,
    );
    assert!(manager.qwen_code_is_active());
    assert!(manager.deepseek_harness_is_active());
    assert!(manager.aider_is_active());
    assert!(
        !manager
            .configure_aider(&models, &config.provider_base_url)
            .expect("Aider refresh should be idempotent")
            .changed
    );

    assert_eq!(
        manager
            .unpersist_qwen_code()
            .expect("Qwen integration should be removed"),
        RemovalOutcome::Removed
    );
    assert_eq!(
        manager
            .unpersist_deepseek_harness()
            .expect("DeepSeek integration should be removed"),
        RemovalOutcome::Removed
    );
    assert_eq!(
        manager
            .unpersist_aider()
            .expect("Aider integration should be removed"),
        RemovalOutcome::Removed
    );
    assert_file_contents([
        (&qwen_path, qwen_original),
        (&deepseek_path, deepseek_original),
        (&aider_settings, aider_settings_original),
        (&aider_metadata, aider_metadata_original),
    ]);
    provider.shutdown().await.expect("provider should stop");
}

fn assert_persisted_catalogs<const N: usize>(paths: [&Path; N], qwen_path: &Path) {
    for path in paths {
        let persisted =
            std::fs::read_to_string(path).expect("persistent configuration should be readable");
        for model in ["qwen3.6", "deepseek-v4-flash", "mimo-v2.5", "gemma4"] {
            assert!(
                persisted.contains(model),
                "{} is missing {model}",
                path.display()
            );
        }
        assert!(!persisted.contains("test-api-key"));
    }
    let qwen = std::fs::read_to_string(qwen_path).expect("Qwen config should remain readable");
    assert!(qwen.contains("\"envKey\": \"NAN_API_KEY\""));
    assert!(qwen.contains("\"selectedType\": \"openai\""));
    assert!(qwen.contains("\"listDirectory\""));
    assert!(qwen.contains("\"enabled\": true"));
}

fn assert_file_contents<const N: usize>(files: [(&Path, &str); N]) {
    for (path, expected) in files {
        assert_eq!(
            std::fs::read_to_string(path).expect("configuration should remain readable"),
            expected
        );
    }
}

#[tokio::test]
async fn qwen_configuration_restores_user_owned_auth_and_model_selections() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let root = tempfile::tempdir().expect("temporary root should exist");
    let home = root.path().join("home");
    let qwen = root.path().join("qwen-home");
    let manager = PersistenceManager::new_with_directories(
        root.path().join("state"),
        &home,
        home.join(".prime/agent"),
        &qwen,
        home.join(".dsh"),
    );
    let qwen_path = qwen.join("settings.json");
    std::fs::create_dir_all(&qwen).expect("Qwen configuration directory should exist");
    let original = concat!(
        "{\n",
        "  \"model\": {\n",
        "    \"name\": \"stale-user-model\",\n",
        "    \"reasoningEffort\": \"high\"\n",
        "  },\n",
        "  \"security\": {\n",
        "    \"auth\": {\n",
        "      \"selectedType\": \"qwen-oauth\"\n",
        "    }\n",
        "  },\n",
        "  \"tools\": {\n",
        "    \"listDirectory\": {\n",
        "      \"enabled\": false\n",
        "    },\n",
        "    \"shell\": {\n",
        "      \"enableInteractiveShell\": true\n",
        "    }\n",
        "  }\n",
        "}\n"
    );
    std::fs::write(&qwen_path, original).expect("Qwen config should be written");
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
    manager
        .configure_qwen_code(&models, &config.provider_base_url)
        .expect("Qwen Code integration should persist");
    assert!(
        std::fs::read_to_string(&qwen_path)
            .expect("Qwen config should remain readable")
            .contains("\"selectedType\": \"openai\"")
    );
    let persisted =
        std::fs::read_to_string(&qwen_path).expect("Qwen config should remain readable");
    assert!(persisted.contains("\"name\": \"qwen3.6\""));
    assert!(persisted.contains("\"reasoningEffort\": \"high\""));
    assert!(persisted.contains("\"enabled\": true"));
    assert!(persisted.contains("\"enableInteractiveShell\": true"));
    assert_eq!(
        manager
            .unpersist_qwen_code()
            .expect("Qwen integration should be removed"),
        RemovalOutcome::Removed
    );
    assert_eq!(
        std::fs::read_to_string(&qwen_path).expect("Qwen config should remain"),
        original
    );

    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}
