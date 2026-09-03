use super::documents::{patch_settings, read_optional};
use super::paths::{ZedPaths, ZedPlatform, settings_path_for_platform};
use super::process::{
    SystemZedProcess, command_is_zed_main, resolve_explicit, validate_passthrough_arguments,
};
use super::session::{begin_session_for_test, begin_session_with_check, restore_session};
use super::{ZedDesktopError, extract_semver, select_model};
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use nan_harness_core::{CodingModelProfile, ReasoningEffort, ReasoningPolicy};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const GATEWAY_URL: &str = "http://127.0.0.1:41234/v1";
type ErrorMatcher = fn(&ZedDesktopError) -> bool;

#[test]
fn platform_paths_follow_zed_conventions() {
    let home = Path::new("/Users/builder");
    let xdg = Path::new("/private/config");
    let app_data = Path::new("/private/windows-app-data");

    assert_eq!(
        settings_path_for_platform(ZedPlatform::Macos, home, None, None)
            .expect("macOS path should resolve"),
        Path::new("/Users/builder/.config/zed/settings.json")
    );
    assert_eq!(
        settings_path_for_platform(ZedPlatform::Linux, home, Some(xdg), None)
            .expect("XDG path should resolve"),
        Path::new("/private/config/zed/settings.json")
    );
    assert_eq!(
        settings_path_for_platform(ZedPlatform::Windows, home, None, Some(app_data))
            .expect("Windows path should resolve"),
        Path::new("/private/windows-app-data/Zed/settings.json")
    );
    assert!(matches!(
        settings_path_for_platform(ZedPlatform::Windows, home, None, None),
        Err(ZedDesktopError::MissingPlatformDirectory)
    ));
}

#[test]
fn jsonc_patch_preserves_user_content_and_builds_the_live_catalog() {
    let source = br#"{
  // Keep this comment and the official provider.
  "language_models": {
    "openai": { "api_url": "https://user.example/v1" },
  },
  "agent": {
    "default_model": { "provider": "openai", "model": "user-model" },
  },
  "theme": "One Dark",
}
"#;
    let models = vec![
        model(
            "qwen3.6",
            "NaN Qwen",
            262_144,
            32_768,
            true,
            ReasoningPolicy::Effort {
                supported: [
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                ],
                default: ReasoningEffort::Medium,
            },
        ),
        model(
            "account-specific-model",
            "NaN Custom",
            65_536,
            8_192,
            false,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        ),
        model(
            "always-reasoning",
            "NaN Always",
            32_768,
            4_096,
            false,
            ReasoningPolicy::AlwaysOn,
        ),
    ];

    let patched =
        patch_settings(Some(source), GATEWAY_URL, &models, "qwen3.6").expect("JSONC should patch");
    let rendered = String::from_utf8(patched.contents.clone()).expect("settings should be UTF-8");
    let value = parse_jsonc(&patched.contents);
    let available = value["language_models"]["openai_compatible"]["nan"]["available_models"]
        .as_array()
        .expect("catalog should be an array");

    assert!(rendered.contains("// Keep this comment and the official provider."));
    assert_eq!(
        value["language_models"]["openai"]["api_url"],
        "https://user.example/v1"
    );
    assert_eq!(
        value["language_models"]["openai_compatible"]["nan"]["api_url"],
        GATEWAY_URL
    );
    assert_eq!(available.len(), models.len());
    assert_eq!(available[0]["name"], "qwen3.6");
    assert_eq!(available[0]["display_name"], "NaN Qwen");
    assert_eq!(available[0]["max_tokens"], 262_144);
    assert_eq!(available[0]["max_output_tokens"], 32_768);
    assert_eq!(available[0]["reasoning_effort"], "medium");
    // Zed 1.18 rejects partial capability objects even for documented defaults.
    assert_eq!(
        available[0]["capabilities"],
        json!({
            "tools": true,
            "images": true,
            "parallel_tool_calls": false,
            "prompt_cache_key": false,
            "chat_completions": true,
            "interleaved_reasoning": false,
            "max_tokens_parameter": true,
        })
    );
    assert!(available[1].get("reasoning_effort").is_none());
    assert!(available[2].get("reasoning_effort").is_none());
    assert_eq!(
        value["agent"]["default_model"],
        json!({"provider": "nan", "model": "qwen3.6"})
    );
    assert_eq!(
        patched.previous_default_model,
        Some(json!({"provider": "openai", "model": "user-model"}))
    );
}

#[test]
fn malformed_or_foreign_settings_fail_closed() {
    let model = model(
        "qwen3.6",
        "NaN Qwen",
        1_024,
        256,
        false,
        ReasoningPolicy::Unknown,
    );
    let cases: &[(&[u8], ErrorMatcher)] = &[
        (b"[]", |error| {
            matches!(error, ZedDesktopError::SettingsRootNotObject)
        }),
        (br#"{"language_models": false}"#, |error| {
            matches!(
                error,
                ZedDesktopError::SettingsFieldNotObject("language_models")
            )
        }),
        (
            br#"{"language_models":{"openai_compatible":[]}}"#,
            |error| {
                matches!(
                    error,
                    ZedDesktopError::SettingsFieldNotObject("language_models.openai_compatible")
                )
            },
        ),
        (br#"{"agent":false}"#, |error| {
            matches!(error, ZedDesktopError::SettingsFieldNotObject("agent"))
        }),
        (br#"{"agent":{"default_model":"model"}}"#, |error| {
            matches!(error, ZedDesktopError::InvalidDefaultModel)
        }),
        (
            br#"{"language_models":{"openai_compatible":{"nan":{}}}}"#,
            |error| matches!(error, ZedDesktopError::UnmanagedProviderConflict),
        ),
    ];

    for (source, expected) in cases {
        let error = patch_settings(
            Some(source),
            GATEWAY_URL,
            std::slice::from_ref(&model),
            "qwen3.6",
        )
        .expect_err("unsafe settings should fail");
        assert!(expected(&error), "unexpected error: {error:?}");
    }
}

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

#[test]
fn startup_race_does_not_touch_settings_or_leave_recovery_state() {
    let fixture = fixture_paths();
    let original = b"{\"theme\":\"race-safe\"}\n";
    write_settings(&fixture.paths, original);

    let error = begin_session_for_test(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        true,
    )
    .expect_err("a process race should fail");

    assert!(matches!(error, ZedDesktopError::AlreadyRunning));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("settings should remain"),
        original
    );
    assert!(!fixture.paths.session_receipt.exists());
    assert!(!fixture.paths.backup_directory.exists());
}

#[test]
fn settings_race_before_write_fails_without_overwriting_the_new_bytes() {
    let fixture = fixture_paths();
    let original = b"{\"theme\":\"before\"}\n";
    let raced = b"{\"theme\":\"changed-by-user\"}\n";
    write_settings(&fixture.paths, original);

    let error = begin_session_with_check(
        &fixture.paths,
        GATEWAY_URL,
        &[generic_model()],
        "qwen3.6",
        || {
            fs::write(&fixture.paths.settings, raced).expect("racing write should succeed");
            Ok(false)
        },
    )
    .expect_err("settings race should fail");

    assert!(matches!(error, ZedDesktopError::SettingsChangedBeforeWrite));
    assert_eq!(
        fs::read(&fixture.paths.settings).expect("racing settings should remain"),
        raced
    );
    assert!(!fixture.paths.session_receipt.exists());
    assert!(!fixture.paths.backup_directory.exists());
}

#[cfg(unix)]
#[test]
fn symlinked_settings_are_rejected_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let fixture = fixture_paths();
    let target = fixture.root.path().join("real-settings.json");
    fs::create_dir_all(
        fixture
            .paths
            .settings
            .parent()
            .expect("settings parent should exist"),
    )
    .expect("settings parent should be created");
    fs::write(&target, b"{\"theme\":\"safe\"}\n").expect("target should be written");
    symlink(&target, &fixture.paths.settings).expect("symlink should be created");

    let error = read_optional(&fixture.paths.settings).expect_err("symlink should fail");
    assert!(matches!(error, ZedDesktopError::State(_)));
    assert_eq!(
        fs::read(&target).expect("target should remain"),
        b"{\"theme\":\"safe\"}\n"
    );
}

#[test]
fn reserved_lifecycle_arguments_are_rejected() {
    for argument in [
        "--foreground",
        "--wait",
        "-w",
        "--user-data-dir",
        "--user-data-dir=/tmp/zed",
    ] {
        assert!(matches!(
            validate_passthrough_arguments(&[argument.to_owned()]),
            Err(ZedDesktopError::ReservedArgument)
        ));
    }
    validate_passthrough_arguments(&["--new".to_owned(), "file.rs".to_owned()])
        .expect("ordinary Zed arguments should pass");
}

#[test]
fn process_detection_distinguishes_main_processes_from_helpers() {
    for command in [
        "/Applications/Zed.app/Contents/MacOS/zed",
        "/usr/local/bin/zed /workspace",
        "zeditor --foreground",
        "C:\\Zed\\zed.exe",
    ] {
        assert!(command_is_zed_main(command), "should detect {command}");
    }
    for command in [
        "/Applications/Zed.app/Contents/MacOS/cli --wait",
        "/Applications/Zed.app/Contents/MacOS/zed --type=gpu-process",
        "unrelated-zed-helper",
    ] {
        assert!(!command_is_zed_main(command), "should ignore {command}");
    }
}

#[cfg(unix)]
#[test]
fn explicit_discovery_supports_each_platform_shape() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().expect("temporary root should exist");
    let plain = root.path().join("zed");
    fs::write(&plain, b"#!/bin/sh\n").expect("executable should be written");
    let mut permissions = fs::metadata(&plain)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&plain, permissions).expect("executable bit should be set");
    assert_eq!(
        resolve_explicit(ZedPlatform::Linux, &plain),
        Some(plain.clone())
    );
    assert_eq!(
        resolve_explicit(ZedPlatform::Windows, &plain),
        Some(plain.clone())
    );

    let app = root.path().join("Zed.app");
    let cli = app.join("Contents/MacOS/cli");
    fs::create_dir_all(cli.parent().expect("CLI parent should exist"))
        .expect("app directories should exist");
    fs::write(&cli, b"#!/bin/sh\n").expect("CLI should be written");
    let mut permissions = fs::metadata(&cli)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&cli, permissions).expect("executable bit should be set");
    assert_eq!(resolve_explicit(ZedPlatform::Macos, &app), Some(cli));
}

#[cfg(unix)]
#[tokio::test]
async fn zed_child_receives_only_the_session_token_as_its_nan_key() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().expect("temporary root should exist");
    let executable = root.path().join("zed");
    let capture = root.path().join("capture.txt");
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).expect("workspace should exist");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$NAN_API_KEY\" \"$@\" > '{}'\n",
            capture.display()
        ),
    )
    .expect("fake Zed should be written");
    let mut permissions = fs::metadata(&executable)
        .expect("metadata should exist")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).expect("executable bit should be set");
    let process = SystemZedProcess::new(Some(executable)).expect("process should resolve");

    let mut child = process
        .spawn(
            &workspace,
            &["--new".to_owned()],
            "launch-scoped-session-token",
        )
        .expect("fake Zed should start");
    assert!(
        child
            .wait()
            .await
            .expect("fake Zed should finish")
            .success()
    );
    let captured = fs::read_to_string(capture).expect("capture should be readable");
    let lines = captured.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "launch-scoped-session-token");
    assert_eq!(lines[1..4], ["--foreground", "--wait", "--new"]);
    assert_eq!(lines[4], workspace.to_string_lossy());
    assert!(!captured.contains("provider-key-marker"));
}

#[test]
fn model_selection_and_version_parsing_are_deterministic() {
    let models = vec![
        model("other", "Other", 1, 1, false, ReasoningPolicy::Unknown),
        generic_model(),
    ];
    assert_eq!(
        select_model(&models, None).expect("default should exist"),
        "qwen3.6"
    );
    assert_eq!(
        select_model(&models, Some("other")).expect("model should exist"),
        "other"
    );
    assert!(matches!(
        select_model(&models, Some("missing")),
        Err(ZedDesktopError::ModelUnavailable { .. })
    ));
    assert_eq!(
        extract_semver("Zed 0.205.4 stable"),
        Some(semver::Version::new(0, 205, 4))
    );
    assert_eq!(extract_semver("unparseable"), None);
}

struct FixturePaths {
    root: tempfile::TempDir,
    paths: ZedPaths,
}

fn fixture_paths() -> FixturePaths {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let paths = ZedPaths::new(
        root.path().join("config/zed/settings.json"),
        root.path().join("state/zed-desktop"),
    )
    .expect("absolute fixture paths should be valid");
    FixturePaths { root, paths }
}

fn write_settings(paths: &ZedPaths, contents: &[u8]) {
    fs::create_dir_all(
        paths
            .settings
            .parent()
            .expect("settings parent should exist"),
    )
    .expect("settings parent should be created");
    fs::write(&paths.settings, contents).expect("settings should be written");
}

fn generic_model() -> CodingModelProfile {
    model(
        "qwen3.6",
        "NaN Qwen",
        262_144,
        32_768,
        true,
        ReasoningPolicy::Unknown,
    )
}

fn model(
    id: &str,
    display_name: &str,
    context_window: u64,
    max_output_tokens: u64,
    image_input: bool,
    reasoning: ReasoningPolicy,
) -> CodingModelProfile {
    let mut profile = CodingModelProfile::generic(id);
    profile.display_name = display_name.to_owned();
    profile.context_window = context_window;
    profile.max_output_tokens = max_output_tokens;
    profile.image_input = image_input;
    profile.reasoning = reasoning;
    profile
}

fn parse_jsonc(contents: &[u8]) -> Value {
    jsonc_parser::parse_to_serde_value(
        std::str::from_utf8(contents).expect("fixture should be UTF-8"),
        &ParseOptions::default(),
    )
    .expect("settings should parse")
}

fn append_root_property(path: &Path, name: &str, value: CstInputValue) {
    let source = fs::read_to_string(path).expect("settings should be readable");
    let root = CstRootNode::parse(&source, &ParseOptions::default())
        .expect("settings should parse as CST");
    root.object_value()
        .expect("settings root should be an object")
        .append(name, value);
    fs::write(path, root.to_string()).expect("settings should update");
}

fn mutate_managed_field(path: &Path, field: &str) {
    let source = fs::read_to_string(path).expect("settings should be readable");
    let root = CstRootNode::parse(&source, &ParseOptions::default())
        .expect("settings should parse as CST");
    let root_object = root.object_value().expect("root should be an object");
    match field {
        "provider" => root_object
            .object_value("language_models")
            .expect("language_models should exist")
            .object_value("openai_compatible")
            .expect("openai_compatible should exist")
            .get("nan")
            .expect("provider should exist")
            .set_value(CstInputValue::Object(vec![])),
        "default_model" => root_object
            .object_value("agent")
            .expect("agent should exist")
            .get("default_model")
            .expect("default should exist")
            .set_value(CstInputValue::Object(vec![
                (
                    "provider".to_owned(),
                    CstInputValue::String("other".to_owned()),
                ),
                (
                    "model".to_owned(),
                    CstInputValue::String("other".to_owned()),
                ),
            ])),
        _ => unreachable!("unknown managed field"),
    }
    fs::write(path, root.to_string()).expect("settings should update");
}
