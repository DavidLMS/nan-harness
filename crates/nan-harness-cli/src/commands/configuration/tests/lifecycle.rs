use super::*;

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
                ManagedSearchStatus {
                    policy: WebSearchPolicy::Auto,
                    managed: false,
                },
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
            .paths_for_search(harness, true)
            .expect("configuration paths should resolve");

        let change = manager
            .configure(
                harness,
                &config,
                &models,
                Some(if harness == HarnessKind::Aider {
                    WebSearchPolicy::Disabled
                } else {
                    WebSearchPolicy::Force
                }),
            )
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
        for entry in fs::read_dir(&state).expect("state directory should be readable") {
            let path = entry.expect("state entry should be readable").path();
            if path.is_file() {
                assert!(
                    !fs::read_to_string(&path)
                        .expect("state file should be UTF-8")
                        .contains("secret-value"),
                    "{harness} leaked its credential into {}",
                    path.display()
                );
            }
        }
        assert_persistent_search_contract(harness, &home);

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
        if harness == HarnessKind::Omp {
            assert_omp_role_routing(&change.paths);
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

fn assert_omp_role_routing(paths: &[PathBuf]) {
    let config_path = paths
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "config.yml" || name == "config.yaml")
        })
        .expect("OMP configuration should include its YAML settings");
    let settings = fs::read_to_string(config_path).expect("OMP configuration should be readable");
    for role in [
        "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
        "advisor",
    ] {
        assert!(
            settings.contains(&format!("{role}: nan/qwen3.6")),
            "OMP role {role} should route through NaN"
        );
    }
}

#[test]
fn legacy_receipts_default_to_auto_without_claiming_search() {
    let receipt: HarnessReceipt = serde_json::from_value(json!({
        "credentialFingerprint": "fingerprint",
        "modelIds": ["qwen3.6"],
        "documents": []
    }))
    .expect("legacy receipt should deserialize");
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(!receipt.search_managed);
}
