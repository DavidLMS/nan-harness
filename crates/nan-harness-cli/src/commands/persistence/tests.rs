use super::{
    PersistenceError, PersistenceManager, PersistentIntegration, RemovalOutcome,
    deepseek_provider_settings, qwen_code_provider,
};
use jsonc_parser::cst::CstRootNode;
use nan_harness_core::{ReasoningSelection, SecretValue, coding_models_from_provider_ids};
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use std::path::Path;

#[test]
fn last_codex_model_is_persisted_separately_from_codex_home() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));

    assert_eq!(
        manager
            .last_codex_model()
            .expect("last Codex model should load"),
        None
    );
    manager
        .save_last_codex_selection("deepseek-v4-flash", Some(ReasoningSelection::Toggle(true)))
        .expect("last Codex selection should save");

    assert_eq!(
        manager
            .last_codex_model()
            .expect("last Codex model should reload"),
        Some("deepseek-v4-flash".to_owned())
    );
    let selection = manager
        .last_codex_selection()
        .expect("last Codex selection should reload")
        .expect("last Codex selection should exist");
    assert_eq!(selection.model, "deepseek-v4-flash");
    assert_eq!(selection.reasoning, Some(ReasoningSelection::Toggle(true)));
    assert!(!root.path().join("home/.codex/config.toml").exists());
    assert!(root.path().join("state/preferences.json").exists());
    assert!(!root.path().join("state/integrations.json").exists());
}

#[test]
fn codex_preferences_do_not_rewrite_integration_receipts() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_directory = root.path().join("state");
    let manager = PersistenceManager::new(&state_directory, root.path().join("home"));
    install_legacy_pi_receipt(&manager, "legacy Pi extension\n");
    let state_path = state_directory.join("integrations.json");
    let before = std::fs::read(&state_path).expect("integration receipts should exist");

    manager
        .save_last_codex_model("deepseek-v4-flash")
        .expect("last Codex model should save");

    let after = std::fs::read(state_path).expect("integration receipts should remain");
    assert_eq!(after, before);
}

#[test]
fn configured_integrations_are_discovered_and_removed_from_receipts() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));

    assert!(
        manager
            .configured_integrations()
            .expect("empty receipts should load")
            .is_empty()
    );
    install_legacy_pi_receipt(&manager, "legacy Pi extension\n");
    install_legacy_prime_receipt(&manager, "legacy Prime extension\n");

    assert_eq!(
        manager
            .configured_integrations()
            .expect("configured receipts should load"),
        vec![PersistentIntegration::Pi, PersistentIntegration::PrimeAgent]
    );
    assert!(manager.integration_is_active(PersistentIntegration::Pi));
    assert!(manager.integration_is_active(PersistentIntegration::PrimeAgent));
    assert_eq!(
        manager
            .unpersist(PersistentIntegration::Pi)
            .expect("Pi integration should be removed"),
        RemovalOutcome::Removed
    );
    assert_eq!(
        manager
            .unpersist(PersistentIntegration::PrimeAgent)
            .expect("Prime integration should be removed"),
        RemovalOutcome::Removed
    );
    assert!(
        manager
            .configured_integrations()
            .expect("updated receipts should load")
            .is_empty()
    );
}

#[test]
fn legacy_codex_preference_remains_readable() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_directory = root.path().join("state");
    std::fs::create_dir_all(&state_directory).expect("state directory should exist");
    std::fs::write(
        state_directory.join("integrations.json"),
        r#"{"schemaVersion":1,"lastCodexModel":"qwen3.6"}"#,
    )
    .expect("legacy state should be written");
    let manager = PersistenceManager::new(&state_directory, root.path().join("home"));

    assert_eq!(
        manager
            .last_codex_model()
            .expect("legacy Codex model should load"),
        Some("qwen3.6".to_owned())
    );
}

#[test]
fn qwen_reasoning_settings_are_model_aware_without_freezing_provider_defaults() {
    let models = coding_models_from_provider_ids(
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "glm5.2",
            "future-stale-model",
        ]
        .map(str::to_owned),
    );
    let root =
        CstRootNode::parse("[]", &jsonc_parser::ParseOptions::default()).expect("valid JSON root");
    root.set_value(qwen_code_provider(&models, "https://api.nan.test/v1"));
    let value = root.to_serde_value().expect("provider should serialize");
    let entries = value
        .as_array()
        .expect("provider catalog should be an array");
    let by_id = |id: &str| {
        entries
            .iter()
            .find(|entry| entry["id"] == id)
            .expect("requested model should be present")
    };

    assert_eq!(
        by_id("glm5.2")["generationConfig"]["reasoning"],
        serde_json::json!(false)
    );
    for id in ["qwen3.6", "deepseek-v4-flash", "future-stale-model"] {
        assert!(
            by_id(id)["generationConfig"].get("reasoning").is_none(),
            "{id} must use provider passthrough until the user makes an explicit choice"
        );
    }
}

#[test]
fn deepseek_serializes_reasoning_capabilities_without_serializing_defaults() {
    let models = coding_models_from_provider_ids(
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "glm5.2",
            "future-stale-model",
        ]
        .map(str::to_owned),
    );
    let settings = deepseek_provider_settings(&models, "https://api.nan.test/v1")
        .expect("DeepSeek settings should serialize");

    let qwen = settings
        .split("        - id: \"qwen3.6\"")
        .nth(1)
        .expect("Qwen block")
        .split("        - id:")
        .next()
        .expect("bounded Qwen block");
    assert!(qwen.contains("reasoning: true"));
    assert!(qwen.contains("supportsReasoningEffort: false"));

    let effort = settings
        .split("        - id: \"deepseek-v4-flash\"")
        .nth(1)
        .expect("effort block")
        .split("        - id:")
        .next()
        .expect("bounded effort block");
    assert!(effort.contains("reasoning: true"));
    assert!(effort.contains("supportsReasoningEffort: true"));

    for id in ["glm5.2", "future-stale-model"] {
        let block = settings
            .split(&format!("        - id: {id:?}"))
            .nth(1)
            .expect("fallback block")
            .split("        - id:")
            .next()
            .expect("bounded fallback block");
        assert!(block.contains("reasoning: false"));
        assert!(block.contains("supportsReasoningEffort: false"));
    }
    assert!(!settings.contains("reasoningEffort:"));
    assert!(!settings.contains("defaultEffort:"));
}

#[test]
fn legacy_pi_configuration_is_reversible_and_detects_manual_changes() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));

    let path = install_legacy_pi_receipt(&manager, "legacy Pi extension\n");
    assert!(manager.pi_is_active());

    std::fs::write(&path, "user change\n").expect("extension should change");
    assert!(matches!(
        manager.unpersist_pi(),
        Err(PersistenceError::ManagedFileChanged(_))
    ));
    std::fs::write(&path, "legacy Pi extension\n").expect("extension should be restored");
    assert_eq!(
        manager
            .unpersist_pi()
            .expect("Pi integration should be removed"),
        RemovalOutcome::Removed
    );
    assert!(!path.exists());
}

#[test]
fn legacy_prime_configuration_uses_its_recorded_path() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let home = root.path().join("home");
    let prime = root.path().join("custom-prime");
    let manager = PersistenceManager::new_with_directories(
        root.path().join("state"),
        &home,
        &prime,
        home.join(".qwen"),
        home.join(".dsh"),
    );

    let path = install_legacy_prime_receipt(&manager, "legacy Prime extension\n");

    assert_eq!(path, prime.join("extensions/nan-provider.js"));
    assert!(manager.prime_agent_is_active());
    assert!(!path.to_string_lossy().ends_with(".mjs"));
    assert_eq!(
        manager
            .unpersist_prime_agent()
            .expect("Prime Agent integration should be removed"),
        RemovalOutcome::Removed
    );
    assert!(!path.exists());
}

fn install_legacy_pi_receipt(manager: &PersistenceManager, content: &str) -> std::path::PathBuf {
    let path = manager
        .home_directory
        .join(super::PI_EXTENSION_RELATIVE_PATH);
    install_legacy_managed_file(manager, path, content, |state, managed| {
        state.pi = Some(managed);
    })
}

fn install_legacy_prime_receipt(manager: &PersistenceManager, content: &str) -> std::path::PathBuf {
    let path = manager.prime_directory.join("extensions/nan-provider.js");
    install_legacy_managed_file(manager, path, content, |state, managed| {
        state.prime_agent = Some(managed);
    })
}

fn install_legacy_managed_file(
    manager: &PersistenceManager,
    path: std::path::PathBuf,
    content: &str,
    assign: impl FnOnce(&mut super::IntegrationState, super::ManagedFile),
) -> std::path::PathBuf {
    std::fs::create_dir_all(path.parent().expect("managed file should have a parent"))
        .expect("managed file directory should exist");
    std::fs::write(&path, content).expect("legacy managed file should be written");
    let mut state = manager.load_state().expect("legacy state should load");
    assign(
        &mut state,
        super::ManagedFile {
            sha256: super::sha256(content.as_bytes()),
            path: Some(path.clone()),
        },
    );
    manager
        .save_state(&state)
        .expect("legacy state should save");
    path
}

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
    let root_node = super::parse_jsonc(&String::from_utf8_lossy(&original), &path)
        .expect("config should parse");
    let root_object = root_node.object_value().expect("root should be object");
    let providers = root_object
        .object_value("provider")
        .expect("providers should exist");
    let models = coding_models_from_provider_ids(["qwen3.6".to_owned(), "mimo-v2.5".to_owned()]);
    let provider = super::opencode_provider(&models, "https://api.nan.builders/v1");
    let hash = super::hash_input_value(&provider).expect("provider should hash");
    providers.append("nan", provider);
    let rendered = root_node.to_string();
    super::write_private_file(&path, rendered.as_bytes(), None).expect("config should update");
    state.opencode = Some(super::ManagedOpenCode {
        provider_sha256: hash,
        file_name: "opencode.jsonc".to_owned(),
        created_file: false,
        created_provider_object: false,
        selected_model: None,
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

    let models = super::discover_models(&config)
        .await
        .expect("model catalog should be discovered");
    let change = manager
        .configure_opencode(&models, &config.provider_base_url)
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
    let aider_settings = home.join(super::AIDER_SETTINGS_RELATIVE_PATH);
    let aider_metadata = home.join(super::AIDER_METADATA_RELATIVE_PATH);
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

    let models = super::discover_models(&config)
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

    let models = super::discover_models(&config)
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
