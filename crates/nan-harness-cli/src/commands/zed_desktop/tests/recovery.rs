use super::super::ZedDesktopError;
use super::super::session::{begin_session_for_test, restore_session};
use super::fixtures::{
    GATEWAY_URL, append_root_property, fixture_paths, generic_model, mutate_managed_field,
    parse_jsonc, write_settings,
};
use jsonc_parser::cst::CstInputValue;
use serde_json::json;
use std::fs;

#[test]
fn exact_restore_is_byte_for_byte_and_idempotent() {
    let fixture = fixture_paths();
    let original = b"{\n  // exact bytes\n  \"theme\": \"One Dark\",\n}\n";
    write_settings(&fixture.paths, original);

    begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    assert_ne!(
        fs::read(&fixture.paths.settings).expect("settings should exist"),
        original
    );
    assert!(restore_session(&fixture.paths).expect("restore should succeed"));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("settings should exist"),
        original
    );
    assert!(!restore_session(&fixture.paths).expect("second restore should be inert"));
}

#[test]
fn receipt_contains_only_recovery_metadata() {
    let fixture = fixture_paths();
    let source = br#"{"theme":"private-theme-marker"}"#;
    write_settings(&fixture.paths, source);

    begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    let receipt =
        fs::read_to_string(&fixture.paths.session_receipt).expect("receipt should be readable");

    assert!(!receipt.contains("real-provider-key"));
    assert!(!receipt.contains("session-token"));
    assert!(!receipt.contains("private-theme-marker"));
    assert!(!receipt.contains(GATEWAY_URL));
    assert!(!receipt.contains("qwen3.6"));
    assert!(receipt.contains("originalSha256"));
    restore_session(&fixture.paths).expect("restore should succeed");
}

#[test]
fn selective_restore_preserves_unrelated_edits_and_previous_model() {
    let fixture = fixture_paths();
    let original = br#"{
  // user content
  "agent": { "default_model": { "provider": "openai", "model": "before" } },
  "theme": "One Dark",
}
"#;
    write_settings(&fixture.paths, original);
    begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    append_root_property(
        &fixture.paths.settings,
        "autosave",
        CstInputValue::String("on_focus_change".into()),
    );

    assert!(restore_session(&fixture.paths).expect("selective restore should succeed"));
    let restored = fs::read(&fixture.paths.settings).expect("settings should remain");
    let rendered = String::from_utf8(restored.clone()).expect("settings should be UTF-8");
    let value = parse_jsonc(&restored);
    assert!(rendered.contains("// user content"));
    assert_eq!(value["theme"], "One Dark");
    assert_eq!(value["autosave"], "on_focus_change");
    assert_eq!(
        value["agent"]["default_model"],
        json!({"provider": "openai", "model": "before"})
    );
    assert!(
        value["language_models"]["openai_compatible"]
            .get("nan")
            .is_none()
    );
}

#[test]
fn created_settings_are_removed_only_when_empty() {
    let empty = fixture_paths();
    begin_session_for_test(
        &empty.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    assert!(restore_session(&empty.paths).expect("restore should succeed"));
    assert!(!empty.paths.settings.exists());

    let edited = fixture_paths();
    begin_session_for_test(
        &edited.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    append_root_property(
        &edited.paths.settings,
        "theme",
        CstInputValue::String("One Dark".into()),
    );
    assert!(restore_session(&edited.paths).expect("restore should succeed"));
    assert_eq!(
        parse_jsonc(&fs::read(&edited.paths.settings).expect("file should remain"))["theme"],
        "One Dark"
    );
}

#[test]
fn changes_to_managed_values_preserve_recovery_state() {
    for managed_field in ["provider", "default_model"] {
        let fixture = fixture_paths();
        write_settings(&fixture.paths, b"{}\n");
        begin_session_for_test(
            &fixture.paths,
            GATEWAY_URL,
            &[generic_model()],
            "qwen3.6",
            false,
        )
        .expect("session should begin");
        mutate_managed_field(&fixture.paths.settings, managed_field);

        let error = restore_session(&fixture.paths).expect_err("managed edits must fail closed");
        assert!(matches!(
            error,
            ZedDesktopError::ManagedConfigurationChanged
        ));
        assert!(fixture.paths.session_receipt.exists());
        assert!(fixture.paths.backup_directory.exists());
    }
}

#[test]
fn invalid_receipts_and_backups_fail_without_destroying_recovery_state() {
    let invalid = fixture_paths();
    fs::create_dir_all(&invalid.paths.state_directory).expect("state directory should exist");
    fs::write(&invalid.paths.session_receipt, b"{\"schemaVersion\":99}\n")
        .expect("invalid receipt should be written");
    assert!(restore_session(&invalid.paths).is_err());
    assert!(invalid.paths.session_receipt.exists());

    let tampered = fixture_paths();
    write_settings(&tampered.paths, b"{\"theme\":\"before\"}\n");
    begin_session_for_test(
        &tampered.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        false,
    )
    .expect("session should begin");
    fs::write(
        tampered.paths.backup_directory.join("settings.backup"),
        b"tampered",
    )
    .expect("backup should be changed");
    let error = restore_session(&tampered.paths).expect_err("tampered backup must fail");
    assert!(matches!(error, ZedDesktopError::BackupHashMismatch));
    assert!(tampered.paths.session_receipt.exists());
}
