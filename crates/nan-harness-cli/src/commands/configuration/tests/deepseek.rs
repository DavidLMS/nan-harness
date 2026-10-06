use super::*;

#[test]
fn deepseek_credentials_preserve_other_refs_through_native_rewrite_refresh_and_remove() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let path = root.path().join(".dsh/.credentials.yaml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "OTHER_API_KEY: synthetic-other\n").unwrap();
    manager
        .configure(
            HarnessKind::DeepSeekHarness,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    let mut native: YamlValue = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(native["version"], 1);
    assert_eq!(native["refs"]["OTHER_API_KEY"], "synthetic-other");
    native["refs"]["ADDED_KEY"] = YamlValue::String("synthetic-added".to_owned());
    fs::write(&path, serde_yaml_ng::to_string(&native).unwrap()).unwrap();
    assert!(manager.is_active(HarnessKind::DeepSeekHarness).unwrap());
    manager
        .configure(
            HarnessKind::DeepSeekHarness,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    manager.remove(HarnessKind::DeepSeekHarness).unwrap();
    let native: YamlValue = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(native["version"], 1);
    assert_eq!(native["refs"]["OTHER_API_KEY"], "synthetic-other");
    assert_eq!(native["refs"]["ADDED_KEY"], "synthetic-added");
    assert!(native["refs"].get("NAN_API_KEY").is_none());
}

#[test]
fn deepseek_normalized_legacy_credential_receipt_recognizes_and_removes_only_owned_ref() {
    let root = tempdir().unwrap();
    let path = root.path().join(".credentials.yaml");
    let begin = "# nan-harness:begin provider-credential";
    let end = "# nan-harness:end provider-credential";
    let block = format!("{begin}\nNAN_API_KEY: \"synthetic-old\"\n{end}\n");
    let receipt = DocumentReceipt::TextBlock(TextBlockReceipt {
        path: path.clone(),
        created_file: false,
        begin: begin.to_owned(),
        end: end.to_owned(),
        block_sha256: sha256(block.as_bytes()),
        active: true,
    });
    fs::write(
        &path,
        "version: 1\nrefs:\n  NAN_API_KEY: synthetic-old\n  OTHER: synthetic-other\n",
    )
    .unwrap();
    assert_eq!(inspect_document(&receipt), ConfigurationHealth::Active);
    let prepared = prepare_removals(std::slice::from_ref(&receipt)).unwrap();
    apply_prepared(&prepared).unwrap();
    let native: YamlValue = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(native["refs"]["OTHER"], "synthetic-other");
    assert!(native["refs"].get("NAN_API_KEY").is_none());
    fs::write(&path, "version: 1\nrefs:\n  NAN_API_KEY: user-changed\n").unwrap();
    assert!(prepare_removals(&[receipt]).is_err());
}
