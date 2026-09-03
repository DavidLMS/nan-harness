use super::super::{
    IntegrationState, ManagedFile, PI_EXTENSION_RELATIVE_PATH, PersistenceError,
    PersistenceManager, PersistentIntegration, RemovalOutcome, sha256,
};

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
    let path = manager.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
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
    assign: impl FnOnce(&mut IntegrationState, ManagedFile),
) -> std::path::PathBuf {
    std::fs::create_dir_all(path.parent().expect("managed file should have a parent"))
        .expect("managed file directory should exist");
    std::fs::write(&path, content).expect("legacy managed file should be written");
    let mut state = manager.load_state().expect("legacy state should load");
    assign(
        &mut state,
        ManagedFile {
            sha256: sha256(content.as_bytes()),
            path: Some(path.clone()),
        },
    );
    manager
        .save_state(&state)
        .expect("legacy state should save");
    path
}
