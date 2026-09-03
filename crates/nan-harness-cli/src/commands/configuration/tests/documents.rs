use super::*;

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
fn json_entries_can_be_added_and_removed_across_refreshes() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.json");
    fs::write(&path, br#"{"userSearch":{"enabled":true}}"#).expect("fixture should be written");
    let inactive = JsonPlan {
        path: path.clone(),
        entries: Vec::new(),
    };
    let initial = prepare_json(&inactive, None).expect("empty plan should prepare");
    let inactive_receipt = json_receipt(&initial);
    assert_eq!(initial.original, initial.replacement);
    apply_prepared(&[initial]).expect("empty plan should apply");

    let active = JsonPlan {
        path: path.clone(),
        entries: vec![exclusive_json(
            &["mcpServers", "nan-search"],
            json!({"command": "nan-harness"}),
        )],
    };
    let enabled =
        prepare_json(&active, Some(&inactive_receipt)).expect("managed entry should be added");
    let enabled_receipt = json_receipt(&enabled);
    apply_prepared(&[enabled]).expect("managed entry should apply");

    let disabled =
        prepare_json(&inactive, Some(&enabled_receipt)).expect("managed entry should be removed");
    let disabled_receipt = json_receipt(&disabled);
    apply_prepared(&[disabled]).expect("managed entry removal should apply");
    assert!(document_is_active(&DocumentReceipt::Json(disabled_receipt)));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(path).expect("user configuration should remain"))
            .expect("user configuration should stay valid"),
        json!({"userSearch": {"enabled": true}})
    );
}

#[test]
fn document_refresh_matches_receipts_by_path_and_accepts_new_documents() {
    let root = tempdir().expect("temporary directory should be created");
    let first = JsonPlan {
        path: root.path().join("first.json"),
        entries: vec![exclusive_json(&["managed"], json!(1))],
    };
    let second = JsonPlan {
        path: root.path().join("second.json"),
        entries: vec![exclusive_json(&["managed"], json!(2))],
    };
    let prepared = prepare_documents(&[DocumentPlan::Json(first.clone())], None)
        .expect("initial document should prepare");
    let receipts = prepared
        .iter()
        .map(|document| document.receipt.clone())
        .collect::<Vec<_>>();
    apply_prepared(&prepared).expect("initial document should apply");

    let refreshed = prepare_documents(
        &[DocumentPlan::Json(second), DocumentPlan::Json(first)],
        Some(&receipts),
    )
    .expect("new document should migrate alongside the existing receipt");
    assert_eq!(refreshed.len(), 2);
}

#[test]
fn yaml_refresh_merges_lists_and_restores_user_search() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("config.yaml");
    fs::write(
        &path,
        "plugins:\n  enabled: [web/tavily]\nweb:\n  search_backend: tavily\n",
    )
    .expect("fixture should be written");
    let plan = YamlPlan {
        path: path.clone(),
        entries: vec![
            YamlEntryPlan {
                path: vec!["plugins".to_owned(), "enabled".to_owned()],
                value: YamlValue::String("web/nan_harness".to_owned()),
                mode: YamlEntryMode::AppendUnique,
            },
            YamlEntryPlan {
                path: vec!["web".to_owned(), "search_backend".to_owned()],
                value: YamlValue::String("nan-harness".to_owned()),
                mode: YamlEntryMode::Override,
            },
        ],
        legacy_block: None,
    };
    let prepared = prepare_yaml(&plan, None).expect("YAML merge should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::Yaml(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("YAML merge should apply");
    let active: YamlValue =
        serde_yaml_ng::from_slice(&fs::read(&path).expect("managed YAML should remain readable"))
            .expect("managed YAML should parse");
    assert_eq!(
        get_yaml_path(&active, &["plugins".to_owned(), "enabled".to_owned()]),
        Some(&YamlValue::Sequence(vec![
            YamlValue::String("web/tavily".to_owned()),
            YamlValue::String("web/nan_harness".to_owned())
        ]))
    );

    let removal = prepare_yaml_removal(&receipt).expect("YAML removal should prepare");
    apply_prepared(&[removal]).expect("YAML removal should apply");
    let restored: YamlValue =
        serde_yaml_ng::from_slice(&fs::read(path).expect("user YAML should remain readable"))
            .expect("restored YAML should parse");
    assert_eq!(
        restored,
        serde_yaml_ng::from_str::<YamlValue>(
            "plugins:\n  enabled: [web/tavily]\nweb:\n  search_backend: tavily\n"
        )
        .expect("fixture YAML should parse")
    );
}

#[test]
fn yaml_plan_migrates_a_managed_text_block() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("config.yaml");
    let legacy = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: Some("model:\n  provider: custom".to_owned()),
        conflicting_keys: vec!["model:".to_owned()],
    };
    let prepared = prepare_text_block(&legacy, None).expect("legacy block should prepare");
    let receipt = prepared.receipt.clone();
    apply_prepared(&[prepared]).expect("legacy block should apply");

    let model = serde_yaml_ng::from_str::<YamlValue>("provider: custom\n")
        .expect("model YAML should parse");
    let migrated = YamlPlan {
        path: path.clone(),
        entries: vec![YamlEntryPlan {
            path: vec!["model".to_owned()],
            value: model,
            mode: YamlEntryMode::Exclusive,
        }],
        legacy_block: Some(LegacyTextBlock {
            begin: legacy.begin,
            end: legacy.end,
        }),
    };
    let prepared = prepare_documents(&[DocumentPlan::Yaml(migrated)], Some(&[receipt]))
        .expect("legacy block should migrate");
    assert!(matches!(prepared[0].receipt, DocumentReceipt::Yaml(_)));
    apply_prepared(&prepared).expect("migrated YAML should apply");
    assert!(
        !fs::read_to_string(&path)
            .expect("migrated YAML should be readable")
            .contains("# begin")
    );
}

#[test]
fn inactive_optional_files_preserve_unmanaged_content_and_conflict_when_enabled() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("plugin.js");
    fs::write(&path, "user plugin\n").expect("user plugin should be written");
    let inactive = ExactFilePlan {
        path: path.clone(),
        payload: None,
    };
    let prepared = prepare_exact_file(&inactive, None).expect("inactive file should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::ExactFile(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("inactive file should preserve user content");
    let removal = prepare_exact_file_removal(&receipt).expect("inactive receipt should remove");
    apply_prepared(&[removal]).expect("inactive removal should preserve user content");
    assert_eq!(
        fs::read_to_string(&path).expect("user plugin should remain"),
        "user plugin\n"
    );

    let active = ExactFilePlan {
        path,
        payload: Some(b"managed plugin\n".to_vec()),
    };
    assert!(matches!(
        prepare_exact_file(&active, Some(&receipt)),
        Err(ConfigurationError::UnmanagedDocumentConflict(_))
    ));
}

#[test]
fn optional_text_blocks_disappear_when_search_is_disabled() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("patch.yml");
    let inactive = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: None,
        conflicting_keys: Vec::new(),
    };
    let prepared = prepare_text_block(&inactive, None).expect("inactive block should prepare");
    let inactive_receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("inactive block should apply");
    assert!(!path.exists());

    let active = TextBlockPlan {
        body: Some("- id: mcp-nan-search".to_owned()),
        ..inactive.clone()
    };
    let prepared =
        prepare_text_block(&active, Some(&inactive_receipt)).expect("active block should prepare");
    let active_receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("active block should apply");
    assert!(path.exists());

    let prepared = prepare_text_block(&inactive, Some(&active_receipt))
        .expect("disabled block should prepare");
    apply_prepared(&[prepared]).expect("disabled block should apply");
    assert!(!path.exists());
}

#[test]
fn managed_documents_detect_manual_changes() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.yaml");
    let plan = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: Some("value: managed".to_owned()),
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
