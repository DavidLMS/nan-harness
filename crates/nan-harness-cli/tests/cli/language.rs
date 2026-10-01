use std::path::Path;
use std::process::{Command, Output};

fn run(state: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", state)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("LANG", "es_ES.UTF-8")
        .env("LC_ALL", "es_ES.UTF-8")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("start terminal command")
}

fn text(output: &[u8]) -> &str {
    std::str::from_utf8(output).expect("UTF-8 terminal output")
}

#[test]
fn language_defaults_to_english_and_only_explicit_selection_persists() {
    let root = tempfile::tempdir().expect("temporary root");
    let state = root.path().join("state");
    let current = run(&state, &["language"]);
    assert!(current.status.success());
    assert!(text(&current.stdout).contains("Current language: en"));
    assert!(
        !state.exists(),
        "reading the language must not create state"
    );
    assert!(text(&run(&state, &["--help"]).stdout).contains("Usage:"));
    assert!(!state.exists(), "help must not initialize configuration");
    let selected = run(&state, &["language", "es"]);
    assert!(selected.status.success(), "{}", text(&selected.stderr));
    assert!(text(&run(&state, &["language"]).stdout).contains("Idioma actual: es"));
    let help = run(&state, &["search", "setup", "--help"]);
    assert!(help.status.success());
    assert!(text(&help.stdout).contains("Uso:"));
    assert!(text(&help.stdout).contains("Opciones:"));
    assert!(!text(&help.stdout).contains("Print help"));
    let invalid = run(&state, &["search", "setup", "--not-a-real-option"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(text(&invalid.stderr).contains("--not-a-real-option"));
    assert!(!text(&invalid.stderr).contains("unexpected argument"));
    let unsupported = run(&state, &["language", "zz"]);
    assert_eq!(unsupported.status.code(), Some(2));
    assert!(text(&run(&state, &["language"]).stdout).contains("Idioma actual: es"));
    assert!(run(&state, &["language", "en"]).status.success());
    assert!(text(&run(&state, &["--help"]).stdout).contains("Usage:"));
    let mut files = std::fs::read_dir(&state)
        .expect("state directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files, ["preferences.json", "preferences.lock"]);
}

#[test]
fn language_preserves_old_preferences_and_machine_status() {
    let root = tempfile::tempdir().expect("temporary root");
    let state = root.path();
    let original = serde_json::json!({"schemaVersion":3,
        "lastSelectionByHarness":{"future-harness":{"model":"synthetic", "reasoning":{"kind":"effort","value":"high"}}},
        "lastSelectionByDesktop":{"future-desktop":{"model":"synthetic-desktop"}}});
    let path = state.join("preferences.json");
    std::fs::write(&path, serde_json::to_vec(&original).expect("fixture JSON"))
        .expect("write fixture");
    let before = run(state, &["search", "status", "--json"]);
    assert!(before.status.success());
    assert!(run(state, &["language", "es"]).status.success());
    let after = run(state, &["search", "status", "--json"]);
    assert!(after.status.success());
    assert_eq!(
        before.stdout, after.stdout,
        "machine output must be locale independent"
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read preferences"))
            .expect("valid preferences");
    assert_eq!(saved["schemaVersion"], 4);
    assert_eq!(saved["language"], "es");
    for key in ["lastSelectionByHarness", "lastSelectionByDesktop"] {
        assert_eq!(
            saved[key].as_object().unwrap().len(),
            original[key].as_object().unwrap().len()
        );
        for (harness, selection) in original[key].as_object().unwrap() {
            assert_eq!(saved[key][harness]["model"], selection["model"]);
            assert_eq!(saved[key][harness]["reasoning"], selection["reasoning"]);
        }
    }
    assert!(!text(&run(state, &["search", "status"]).stdout).contains("disabled"));
}

#[test]
fn unsupported_or_corrupt_saved_language_falls_back_without_overwriting() {
    let root = tempfile::tempdir().expect("temporary root");
    let path = root.path().join("preferences.json");
    for original in [
        r#"{"schemaVersion":4,"language":"future-language","lastSelectionByHarness":{},"lastSelectionByDesktop":{}}"#,
        "{broken",
    ] {
        std::fs::write(&path, original).expect("write fixture");
        assert!(text(&run(root.path(), &["--help"]).stdout).contains("Usage:"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read unchanged fixture"),
            original
        );
    }
    assert!(!run(root.path(), &["language", "es"]).status.success());
    assert_eq!(
        std::fs::read_to_string(&path).expect("read preserved fixture"),
        "{broken"
    );
}
