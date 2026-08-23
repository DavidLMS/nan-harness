use super::documents::{prepare_json, prepare_json_removal, prepare_text_block};
use super::*;
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use tempfile::tempdir;

#[test]
fn all_native_configurations_are_reversible_and_keep_secrets_out_of_receipts() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);
    let models = test_models();

    for harness in SUPPORTED_HARNESSES {
        let plans = manager
            .plans_for(
                harness,
                "secret-value",
                "https://api.nan.test/v1",
                &models,
                "qwen3.6",
            )
            .expect("native configuration plan should build");
        let prepared =
            prepare_documents(&plans, None).expect("native configuration should prepare");
        let receipts = prepared
            .iter()
            .map(|document| document.receipt.clone())
            .collect::<Vec<_>>();
        let serialized =
            serde_json::to_string(&receipts).expect("configuration receipts should serialize");
        assert!(
            !serialized.contains("secret-value"),
            "{harness} leaked its credential into a receipt"
        );

        apply_prepared(&prepared).expect("native configuration should apply");
        assert!(
            receipts.iter().all(document_is_active),
            "{harness} configuration was not recognized as active"
        );
        let removals =
            prepare_removals(&receipts).expect("native configuration removal should prepare");
        apply_prepared(&removals).expect("native configuration should be removed");
        assert!(
            receipts
                .iter()
                .all(|receipt| !receipt_path(receipt).exists()),
            "{harness} left a newly created configuration document behind"
        );
    }
}

#[test]
fn integrated_configuration_lifecycle_covers_every_supported_harness() {
    let models = test_models();
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://api.nan.test/v1".to_owned()),
            nan_api_key: Some(
                SecretValue::new("secret-value").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve");

    for harness in SUPPORTED_HARNESSES {
        let root = tempdir().expect("temporary directory should be created");
        let home = root.path().join("home");
        let state = root.path().join("state");
        fs::create_dir_all(&home).expect("home should be created");
        let manager = ConfigurationManager::new(&state, &home);
        let expected_paths = manager
            .paths_for(harness)
            .expect("configuration paths should resolve");

        let change = manager
            .configure(harness, &config, &models)
            .expect("native configuration should apply");
        assert!(change.changed, "{harness} should change a clean home");
        assert_eq!(change.paths, expected_paths, "{harness} path list changed");
        assert!(
            change.paths.iter().all(|path| path.exists()),
            "{harness} omitted a written path"
        );
        assert!(manager.is_active(harness).expect("status should resolve"));
        assert!(
            !fs::read_to_string(state.join(STATE_FILE_NAME))
                .expect("receipt should be readable")
                .contains("secret-value"),
            "{harness} leaked its credential into the receipt"
        );

        if matches!(harness, HarnessKind::Pi | HarnessKind::PrimeAgent) {
            let models_path = change
                .paths
                .iter()
                .find(|path| path.file_name().is_some_and(|name| name == "models.json"))
                .expect("Pi-compatible configuration should include models.json");
            let catalog = fs::read_to_string(models_path)
                .expect("Pi-compatible model catalog should be readable");
            assert!(
                catalog.contains(r#""apiKey": "NAN_API_KEY""#),
                "{harness} should provide the required environment credential fallback"
            );
            assert!(
                !catalog.contains("secret-value"),
                "{harness} leaked its credential into the model catalog"
            );
        }

        assert_eq!(
            manager
                .remove(harness)
                .expect("configuration should remove"),
            RemovalOutcome::Removed
        );
        assert!(
            change.paths.iter().all(|path| !path.exists()),
            "{harness} left a clean-home configuration behind"
        );
        assert!(
            !manager
                .is_configured(harness)
                .expect("status should resolve")
        );
    }
}

#[test]
fn json_refresh_rotates_secrets_and_restores_previous_defaults() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.json");
    fs::write(&path, br#"{"defaultModel":"user-model"}"#).expect("fixture should be written");
    let first = JsonPlan {
        path: path.clone(),
        entries: vec![
            override_json(&["defaultModel"], json!("qwen3.6")),
            exclusive_json(&["providers", "nan"], json!({"key": "first-secret"})),
        ],
    };
    let prepared = prepare_json(&first, None).expect("configuration should prepare");
    let first_receipt = json_receipt(&prepared);
    apply_prepared(&[prepared]).expect("configuration should apply");

    let refreshed = JsonPlan {
        path: path.clone(),
        entries: vec![
            override_json(&["defaultModel"], json!("qwen3.6")),
            exclusive_json(&["providers", "nan"], json!({"key": "second-secret"})),
        ],
    };
    let prepared =
        prepare_json(&refreshed, Some(&first_receipt)).expect("configuration should refresh");
    let refreshed_receipt = json_receipt(&prepared);
    assert!(
        !serde_json::to_string(&refreshed_receipt)
            .expect("receipt should serialize")
            .contains("second-secret")
    );
    apply_prepared(&[prepared]).expect("refresh should apply");
    let removal =
        prepare_json_removal(&refreshed_receipt).expect("configuration removal should prepare");
    apply_prepared(&[removal]).expect("configuration removal should apply");
    assert_eq!(
        fs::read_to_string(path).expect("user configuration should remain"),
        "{\n  \"defaultModel\": \"user-model\"\n}"
    );
}

#[test]
fn managed_documents_detect_manual_changes() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.yaml");
    let plan = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: "value: managed".to_owned(),
        conflicting_keys: vec!["value:".to_owned()],
    };
    let prepared = prepare_text_block(&plan, None).expect("block should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("block should apply");
    fs::write(&path, "# begin\nvalue: changed\n# end\n").expect("managed block should be changed");
    assert!(matches!(
        prepare_text_block(&plan, Some(&receipt)),
        Err(ConfigurationError::ManagedDocumentChanged(_))
    ));
}

#[test]
fn confirmation_paths_include_native_credentials_catalogs_and_defaults() {
    let root = tempdir().expect("temporary directory should be created");
    let state = root.path().join("state");
    let home = root.path().join("home");
    let manager = ConfigurationManager::new(&state, &home);

    let cases = [
        (HarnessKind::OpenCode, vec!["auth.json", "opencode.json"]),
        (HarnessKind::QwenCode, vec![".env", "settings.json"]),
        (
            HarnessKind::DeepSeekHarness,
            vec![".credentials.yaml", "settings.yaml"],
        ),
        (
            HarnessKind::Aider,
            vec![
                ".aider.conf.yml",
                ".aider.model.metadata.json",
                ".aider.model.settings.yml",
            ],
        ),
    ];

    for (harness, expected_names) in cases {
        let paths = manager
            .paths_for(harness)
            .expect("configuration paths should resolve");
        let names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .collect::<BTreeSet<_>>();
        for expected in expected_names {
            assert!(
                names.contains(expected),
                "{harness} confirmation omitted {expected}: {paths:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn newly_written_native_configuration_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("auth.json");
    let plan = JsonPlan {
        path: path.clone(),
        entries: vec![exclusive_json(&["nan"], json!({"key": "secret-value"}))],
    };
    let prepared = prepare_json(&plan, None).expect("configuration should prepare");
    apply_prepared(&[prepared]).expect("configuration should apply");
    assert_eq!(
        fs::metadata(path)
            .expect("configuration metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn test_models() -> Vec<CodingModelProfile> {
    vec![
        CodingModelProfile::generic("qwen3.6"),
        CodingModelProfile::generic("future-model"),
    ]
}

fn receipt_path(receipt: &DocumentReceipt) -> &Path {
    match receipt {
        DocumentReceipt::Json(receipt) => &receipt.path,
        DocumentReceipt::TextBlock(receipt) => &receipt.path,
        DocumentReceipt::ExactFile(receipt) => &receipt.path,
        DocumentReceipt::Toml(receipt) => &receipt.path,
    }
}

fn json_receipt(document: &PreparedDocument) -> JsonReceipt {
    match &document.receipt {
        DocumentReceipt::Json(receipt) => receipt.clone(),
        _ => panic!("expected a JSON receipt"),
    }
}
