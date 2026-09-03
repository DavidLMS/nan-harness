use super::*;

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
            .paths_for_search(harness, true)
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
