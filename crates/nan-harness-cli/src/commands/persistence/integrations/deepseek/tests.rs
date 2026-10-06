use super::*;
use nan_harness_core::coding_model_profile;

fn models() -> Vec<CodingModelProfile> {
    ["qwen3.6", "glm5.3-flash"]
        .into_iter()
        .map(|id| coding_model_profile(id).unwrap())
        .collect()
}

#[test]
fn profiles_refresh_semantically_and_restore_unrelated_fields() {
    let root = tempfile::tempdir().unwrap();
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
    let path = manager
        .deepseek_directory
        .join("profiles/headless/cordis.patch.yml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "# user comment\n- id: llm-pi-ai\n  config:\n    customSetting: keep\n    providers:\n      personal: {apiKeyEnv: PERSONAL_KEY}\n- id: agent-default-model\n  config: {provider: personal, model: original, other: keep}\n";
    std::fs::write(&path, original).unwrap();
    manager
        .configure_deepseek_harness(&models(), "https://nan.test/v1")
        .unwrap();
    assert!(manager.deepseek_harness_is_active());
    let mut document = cordis::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    document[0]["config"]["userAdded"] = Value::String("keep".to_owned());
    std::fs::write(&path, cordis::render(&document).unwrap()).unwrap();
    assert!(manager.deepseek_harness_is_active());
    manager
        .configure_deepseek_harness(&models()[1..], "https://nan.test/v2")
        .unwrap();
    let document = cordis::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    assert!(
        document[0]["config"]["providers"]
            .get("nan-harness-budgeted")
            .is_none()
    );
    manager.unpersist_deepseek_harness().unwrap();
    let document = cordis::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    assert_eq!(
        document[0]["config"]["providers"]["personal"]["apiKeyEnv"],
        "PERSONAL_KEY"
    );
    assert_eq!(document[0]["config"]["userAdded"], "keep");
    assert_eq!(document[1]["config"]["model"], "original");
    assert!(
        document[0]["config"]["providers"]
            .get("nan-harness")
            .is_none()
    );
}

#[test]
fn imported_ownership_keeps_user_siblings_in_the_rollback_baseline() {
    let path = Path::new("cordis.patch.yml");
    let source = "- id: llm-pi-ai\n  name: plugin\n  config:\n    providers:\n      nan-harness: {models: [{id: old}]}\n      personal: {models: [{id: user}]}\n- id: agent-default-model\n  config: {provider: nan-harness, model: old, userSetting: keep}\n";
    let legacy: Value = serde_yaml_ng::from_str("llm-pi-ai:\n  providers:\n    nan-harness: {models: [{id: old}]}\nagent-default-model: {provider: nan-harness, model: old}\n").unwrap();
    let document = cordis::parse(source, path).unwrap();
    let mut receipt = cordis::receipt(path, Some(source), None);
    adopt_imported(&document, &mut receipt, &legacy, source).unwrap();
    let baseline = cordis::parse(receipt.original.as_ref().unwrap(), path).unwrap();
    assert_eq!(
        baseline[0]["config"]["providers"]["personal"]["models"][0]["id"],
        "user"
    );
    assert!(
        baseline[0]["config"]["providers"]
            .get("nan-harness")
            .is_none()
    );
    assert_eq!(baseline[1]["config"]["userSetting"], "keep");
}

#[test]
fn changed_owned_provider_fails_before_any_publication() {
    let root = tempfile::tempdir().unwrap();
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
    manager
        .configure_deepseek_harness(&models(), "https://nan.test/v1")
        .unwrap();
    let path = manager
        .deepseek_directory
        .join("profiles/headless/cordis.patch.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace("https://nan.test/v1", "https://user.test/v1");
    std::fs::write(&path, &source).unwrap();
    assert!(
        manager
            .configure_deepseek_harness(&models(), "https://nan.test/v2")
            .is_err()
    );
    assert!(manager.unpersist_deepseek_harness().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[test]
fn unsupported_builtin_bundle_is_actionable_and_never_partially_configured() {
    let root = tempfile::tempdir().unwrap();
    let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
    let directory = manager.deepseek_directory.join("profiles/web");
    std::fs::create_dir_all(&directory).unwrap();
    let manifest = r#"{"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","custom-bundle"]}}}"#;
    std::fs::write(directory.join("package.json"), manifest).unwrap();
    let error = manager
        .configure_deepseek_harness(&models(), "https://nan.test/v1")
        .unwrap_err();
    assert!(matches!(
        error,
        PersistenceError::UnsupportedDeepSeekProfile(_)
    ));
    assert!(!directory.join("cordis.patch.yml").exists());
    assert!(!manager.deepseek_directory.join("profiles/acp").exists());
}

#[test]
fn legacy_import_migrates_and_removes_without_reviving_nan_or_deleting_user_state() {
    for refresh in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
        let path = manager
            .deepseek_directory
            .join("profiles/headless/cordis.patch.yml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = "agent-default-model: {provider: nan-harness, model: old}\nllm-pi-ai:\n  providers:\n    nan-harness: {models: [{id: old}]}\n";
        let block = format!("{DEEPSEEK_BLOCK_BEGIN}\n{legacy}{DEEPSEEK_BLOCK_END}\n");
        let old_path = manager.deepseek_directory.join("settings.yaml");
        std::fs::write(old_path.with_extension("yaml.imported"), &block).unwrap();
        std::fs::write(&path, "- id: llm-pi-ai\n  config:\n    providers:\n      nan-harness: {models: [{id: old}]}\n      personal: {models: [{id: user}]}\n- id: agent-default-model\n  config: {provider: nan-harness, model: old}\n").unwrap();
        let (mut state, original) = manager.prepare_state().unwrap();
        state.deepseek_harness = Some(super::super::super::ManagedBlock {
            path: old_path,
            block_sha256: sha256(block.as_bytes()),
            created_file: true,
            added_separator: false,
        });
        let files =
            PersistenceManager::prepare_integration_files(Vec::new(), &state, original).unwrap();
        manager.publish_configuration_files(&files).unwrap();
        if refresh {
            manager
                .configure_deepseek_harness(&models(), "https://nan.test/v1")
                .unwrap();
        }
        manager.unpersist_deepseek_harness().unwrap();
        let document = cordis::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
        let row = cordis::entry(&document, "llm-pi-ai", &path)
            .unwrap()
            .unwrap();
        assert_eq!(
            row["config"]["providers"]["personal"]["models"][0]["id"],
            "user"
        );
        assert!(row["config"]["providers"].get("nan-harness").is_none());
        assert!(
            cordis::entry(&document, "agent-default-model", &path)
                .unwrap()
                .is_none()
        );
    }
}
