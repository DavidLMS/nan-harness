use super::super::{PersistenceError, PersistenceManager};
use nan_harness_core::{DesktopHarnessKind, HarnessKind, ReasoningEffort, ReasoningSelection};

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
        .save_last_selection(
            HarnessKind::Codex,
            "deepseek-v4-flash",
            Some(ReasoningSelection::Toggle(true)),
        )
        .expect("last Codex selection should save");

    assert_eq!(
        manager
            .last_codex_model()
            .expect("last Codex model should reload"),
        Some("deepseek-v4-flash".to_owned())
    );
    let selection = manager
        .last_selection(HarnessKind::Codex)
        .expect("last Codex selection should reload")
        .expect("last Codex selection should exist");
    assert_eq!(selection.model, "deepseek-v4-flash");
    assert_eq!(selection.reasoning, Some(ReasoningSelection::Toggle(true)));
    assert!(!root.path().join("home/.codex/config.toml").exists());
    assert!(root.path().join("state/preferences.json").exists());
    assert!(!root.path().join("state/integrations.json").exists());
}

#[test]
fn preferences_migrate_strict_v1_in_memory_and_write_v4_only_after_save() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_directory = root.path().join("state");
    std::fs::create_dir_all(&state_directory).expect("state directory should exist");
    let preferences_path = state_directory.join("preferences.json");
    let v1 = br#"{
  "schemaVersion": 1,
  "lastCodexModel": "glm5.2",
  "lastCodexReasoning": { "kind": "effort", "value": "high" }
}"#;
    std::fs::write(&preferences_path, v1).expect("v1 preferences should write");
    let manager = PersistenceManager::new(&state_directory, root.path().join("home"));

    let migrated = manager
        .last_selection(HarnessKind::Codex)
        .expect("v1 preferences should migrate")
        .expect("Codex selection should exist");
    assert_eq!(migrated.model, "glm5.2");
    assert_eq!(
        migrated.reasoning,
        Some(ReasoningSelection::Effort(ReasoningEffort::High))
    );
    assert_eq!(
        std::fs::read(&preferences_path).expect("preferences should remain readable"),
        v1,
        "reading v1 must not rewrite it"
    );

    manager
        .save_last_selection(HarnessKind::Fx, "future-fx-model", None)
        .expect("a later successful selection should save");
    let written: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&preferences_path).expect("v4 preferences should be readable"),
    )
    .expect("v4 preferences should be JSON");
    assert_eq!(written["schemaVersion"], 4);
    assert_eq!(
        written["lastSelectionByHarness"]["codex"]["model"],
        "glm5.2"
    );
    assert_eq!(
        written["lastSelectionByHarness"]["fx"]["model"],
        "future-fx-model"
    );

    std::fs::write(
        &preferences_path,
        r#"{"schemaVersion":1,"lastCodexModel":"qwen3.6","unexpected":true}"#,
    )
    .expect("strict v1 fixture should write");
    assert!(matches!(
        manager.last_selection(HarnessKind::Codex),
        Err(PersistenceError::ParsePreferences(_))
    ));
}

#[test]
fn preferences_v4_round_trip_stable_and_desktop_harnesses_and_reject_future_schemas() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_directory = root.path().join("state");
    let manager = PersistenceManager::new(&state_directory, root.path().join("home"));

    for (index, kind) in HarnessKind::ALL.into_iter().enumerate() {
        manager
            .save_last_selection(kind, &format!("model-{index}"), None)
            .expect("harness selection should save");
    }
    for (index, kind) in DesktopHarnessKind::ALL.into_iter().enumerate() {
        manager
            .save_last_desktop_selection(kind, &format!("desktop-model-{index}"))
            .expect("Desktop selection should save");
        assert_eq!(
            manager
                .last_desktop_selection(kind)
                .expect("Desktop selection should load")
                .expect("Desktop selection should exist")
                .model,
            format!("desktop-model-{index}")
        );
    }
    for (index, kind) in HarnessKind::ALL.into_iter().enumerate() {
        assert_eq!(
            manager
                .last_selection(kind)
                .expect("harness selection should load")
                .expect("harness selection should exist")
                .model,
            format!("model-{index}"),
            "selection for {kind} should round trip"
        );
    }
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state_directory.join("preferences.json"))
            .expect("preferences should be readable"),
    )
    .expect("preferences should be valid JSON");
    assert_eq!(
        value["lastSelectionByHarness"]
            .as_object()
            .expect("harness map should be an object")
            .len(),
        HarnessKind::ALL.len()
    );
    assert_eq!(
        value["lastSelectionByDesktop"]
            .as_object()
            .expect("Desktop map should be an object")
            .len(),
        DesktopHarnessKind::ALL.len()
    );

    std::fs::write(
        state_directory.join("preferences.json"),
        r#"{"schemaVersion":2,"lastSelectionByHarness":{},"unexpected":true}"#,
    )
    .expect("strict v2 preferences should write");
    assert!(matches!(
        manager.last_selection(HarnessKind::Codex),
        Err(PersistenceError::ParsePreferences(_))
    ));

    std::fs::write(
        state_directory.join("preferences.json"),
        r#"{"schemaVersion":5,"lastSelectionByHarness":{},"lastSelectionByDesktop":{}}"#,
    )
    .expect("future preferences should write");
    assert!(matches!(
        manager.last_selection(HarnessKind::Codex),
        Err(PersistenceError::UnsupportedPreferencesSchema(5))
    ));
}

#[test]
fn preferences_preserve_unknown_selection_keys_when_saving() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_directory = root.path().join("state");
    std::fs::create_dir_all(&state_directory).expect("state directory should exist");
    std::fs::write(
        state_directory.join("preferences.json"),
        r#"{
  "schemaVersion": 3,
  "lastSelectionByHarness": {
    "future-harness": {"model": "future-harness-model", "reasoning": null}
  },
  "lastSelectionByDesktop": {
    "zed-desktop": {"model": "glm5.2", "reasoning": null},
    "future-desktop": {"model": "future-model", "reasoning": null}
  }
}"#,
    )
    .expect("future desktop preference should be written");
    let manager = PersistenceManager::new(&state_directory, root.path().join("home"));

    manager
        .save_last_selection(HarnessKind::Codex, "glm5.3-flash", None)
        .expect("known harness preference should save");

    let preferences: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state_directory.join("preferences.json"))
            .expect("preferences should remain readable"),
    )
    .expect("preferences should remain valid JSON");
    assert_eq!(
        preferences["lastSelectionByDesktop"]["future-desktop"]["model"],
        "future-model"
    );
    assert_eq!(
        preferences["lastSelectionByDesktop"]["zed-desktop"]["model"],
        "glm5.2"
    );
    assert_eq!(
        preferences["lastSelectionByHarness"]["future-harness"]["model"],
        "future-harness-model"
    );
    assert_eq!(
        preferences["lastSelectionByHarness"]["codex"]["model"],
        "glm5.3-flash"
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
fn language_read_is_inert_and_invalid_preferences_are_preserved() {
    use super::super::PreferencesStore;
    let root = tempfile::tempdir().expect("temporary directory");
    let directory = root.path().join("preferences");
    let store = PreferencesStore::new(directory.clone());
    assert_eq!(store.language().expect("missing preference"), None);
    assert!(!directory.exists());
    std::fs::create_dir(&directory).expect("create directory");
    let path = directory.join("preferences.json");
    let invalid = b"{ invalid preferences";
    std::fs::write(&path, invalid).expect("write invalid fixture");
    assert!(store.set_language("es").is_err());
    assert_eq!(std::fs::read(&path).expect("read fixture"), invalid);
}

#[test]
fn concurrent_language_and_model_writes_preserve_both_preferences() {
    use super::super::PreferencesStore;
    use std::sync::{Arc, Barrier};
    let root = tempfile::tempdir().expect("temporary directory");
    let directory = root.path().join("state");
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        let ready = Arc::clone(&barrier);
        let state = directory.clone();
        scope.spawn(move || {
            ready.wait();
            PreferencesStore::new(state)
                .set_language("es")
                .expect("save language");
        });
        let ready = Arc::clone(&barrier);
        let state = directory.clone();
        let home = root.path().join("home");
        scope.spawn(move || {
            ready.wait();
            PersistenceManager::new(state, home)
                .save_last_selection(HarnessKind::Codex, "synthetic-model", None)
                .expect("save model");
        });
        barrier.wait();
    });
    assert_eq!(
        PreferencesStore::new(directory.clone())
            .language()
            .expect("load language")
            .as_deref(),
        Some("es")
    );
    let manager = PersistenceManager::new(directory, root.path().join("home"));
    assert_eq!(
        manager.last_codex_model().expect("load model").as_deref(),
        Some("synthetic-model")
    );
}

#[test]
fn concurrent_processes_preserve_language_and_model_selections() {
    use super::super::PreferencesStore;
    use std::process::{Command, Stdio};
    const ROLE: &str = "NAN_TEST_PREFERENCES_ROLE";
    const DIRECTORY: &str = "NAN_TEST_PREFERENCES_DIRECTORY";
    if let Ok(role) = std::env::var(ROLE) {
        let directory =
            std::path::PathBuf::from(std::env::var_os(DIRECTORY).expect("child directory"));
        for _ in 0..12 {
            if role == "language" {
                PreferencesStore::new(directory.clone())
                    .set_language("es")
                    .expect("save child language");
            } else {
                PersistenceManager::new(&directory, directory.join("home"))
                    .save_last_selection(
                        HarnessKind::Codex,
                        "concurrent-model",
                        Some(ReasoningSelection::Toggle(true)),
                    )
                    .expect("save child model");
            }
        }
        return;
    }
    let root = tempfile::tempdir().expect("temporary directory");
    let mut children = ["language", "model"].map(|role| {
        Command::new(std::env::current_exe().expect("test executable"))
            .arg("concurrent_processes_preserve_language_and_model_selections")
            .arg("--test-threads=1")
            .env(ROLE, role)
            .env(DIRECTORY, root.path())
            .stdout(Stdio::null())
            .spawn()
            .expect("start preference writer")
    });
    for child in &mut children {
        assert!(child.wait().expect("writer exit").success());
    }
    let language = PreferencesStore::new(root.path().to_path_buf())
        .language()
        .expect("load language");
    assert_eq!(language.as_deref(), Some("es"));
    let selected = PersistenceManager::new(root.path(), root.path().join("home"))
        .last_selection(HarnessKind::Codex)
        .expect("load model")
        .expect("selected model");
    assert_eq!(selected.model, "concurrent-model");
    assert_eq!(selected.reasoning, Some(ReasoningSelection::Toggle(true)));
}

#[test]
fn failed_language_lock_preserves_preferences_and_other_configuration() {
    use super::super::PreferencesStore;
    let root = tempfile::tempdir().expect("temporary directory");
    let preferences = root.path().join("preferences.json");
    let original =
        br#"{"schemaVersion":3,"lastSelectionByHarness":{"codex":{"model":"existing"}}}"#;
    std::fs::write(&preferences, original).expect("write preferences");
    std::fs::write(root.path().join("integrations.json"), b"user-owned")
        .expect("write other state");
    std::fs::create_dir(root.path().join("preferences.lock")).expect("block the lock path");
    assert!(
        PreferencesStore::new(root.path().to_path_buf())
            .set_language("es")
            .is_err()
    );
    assert_eq!(
        std::fs::read(preferences).expect("read preserved preferences"),
        original
    );
    assert_eq!(
        std::fs::read(root.path().join("integrations.json")).expect("read other state"),
        b"user-owned"
    );
}
