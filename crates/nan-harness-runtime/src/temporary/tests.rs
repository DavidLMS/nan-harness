use super::TemporaryWorkspace;
use super::lifecycle::ensure_configuration_directory;
use super::paths::resolve_overlay_source;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CODEX_HOME_PLACEHOLDER, ConfigurationOverlay, LaunchScopedFile, OverlayFile,
    OverlayFilePolicy, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use std::fs;

#[cfg(unix)]
#[test]
fn missing_configuration_directories_are_created_private() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temporary home should exist");
    let path = home.path().join("nested/configuration");

    ensure_configuration_directory(&path, "test-config")
        .expect("configuration directories should be created");

    for directory in [home.path().join("nested"), path] {
        let mode = fs::metadata(&directory)
            .expect("configuration directory metadata should exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[cfg(unix)]
#[test]
fn preexisting_configuration_directory_permissions_are_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temporary home should exist");
    let path = home.path().join("configuration");
    fs::create_dir(&path).expect("configuration directory should exist");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("configuration directory should be permissive");

    ensure_configuration_directory(&path, "test-config")
        .expect("preexisting configuration directory should be accepted");

    assert_eq!(
        fs::metadata(&path)
            .expect("configuration directory metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn codex_overlay_source_prefers_the_configured_home() {
    let user_home = tempfile::tempdir().expect("temporary user home should exist");
    let codex_home = tempfile::tempdir().expect("temporary Codex home should exist");

    assert_eq!(
        resolve_overlay_source(
            CODEX_HOME_PLACEHOLDER,
            user_home.path(),
            Some(codex_home.path().as_os_str()),
        ),
        codex_home.path()
    );
    assert_eq!(
        resolve_overlay_source(CODEX_HOME_PLACEHOLDER, user_home.path(), None),
        user_home.path().join(".codex")
    );
}

#[test]
fn launch_scoped_profiles_are_private_and_removed_on_drop() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    fs::write(codex_home.join("config.toml"), "notify = [\"true\"]\n")
        .expect("base config should exist");
    let files = [codex_profile("launch_01scopedfile")];

    let workspace = TemporaryWorkspace::materialize_with_home_and_scoped(
        &[],
        &[],
        &files,
        home.path(),
        |_, content| Ok(content.to_owned()),
    )
    .expect("profile should materialize");
    let profile = workspace
        .path("codex-profile")
        .expect("profile path should exist")
        .to_path_buf();
    let lock = profile.with_file_name(format!(
        "{}.lock",
        profile
            .file_name()
            .expect("profile name should exist")
            .to_string_lossy()
    ));

    assert_eq!(
        fs::read_to_string(&profile).expect("profile should be readable"),
        "model = \"qwen3.6\"\n"
    );
    assert!(lock.exists());
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml"))
            .expect("base config should remain readable"),
        "notify = [\"true\"]\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&profile)
                .expect("profile metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(&profile)
        .expect("launch-scoped profile should have a private protected DACL");

    drop(workspace);
    assert!(!profile.exists());
    assert!(!lock.exists());
}

#[test]
fn launch_scoped_profile_cleanup_preserves_active_launches() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let stale = codex_home.join("nan-harness-launch_01staleprofile.config.toml");
    let stale_lock = codex_home.join("nan-harness-launch_01staleprofile.config.toml.lock");
    fs::write(&stale, "stale").expect("stale profile should exist");
    fs::write(&stale_lock, "").expect("stale lock should exist");

    let first_files = [codex_profile("launch_01firstactive")];
    let first = TemporaryWorkspace::materialize_with_home_and_scoped(
        &[],
        &[],
        &first_files,
        home.path(),
        |_, content| Ok(content.to_owned()),
    )
    .expect("first profile should materialize");
    let first_profile = first
        .path("codex-profile")
        .expect("first profile should exist")
        .to_path_buf();
    assert!(!stale.exists());
    assert!(!stale_lock.exists());

    let second_files = [codex_profile("launch_01secondactive")];
    let second = TemporaryWorkspace::materialize_with_home_and_scoped(
        &[],
        &[],
        &second_files,
        home.path(),
        |_, content| Ok(content.to_owned()),
    )
    .expect("second profile should materialize");
    assert!(first_profile.exists());

    drop(second);
    assert!(first_profile.exists());
    drop(first);
    assert!(!first_profile.exists());
}

fn codex_profile(launch_id: &str) -> LaunchScopedFile {
    LaunchScopedFile {
        id: "codex-profile".to_owned(),
        directory: format!("{USER_HOME_PLACEHOLDER}/.codex"),
        file_name: format!("nan-harness-{launch_id}.config.toml"),
        ownership_prefix: "nan-harness-launch_".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: "model = \"qwen3.6\"\n".to_owned(),
        lifecycle: ArtifactLifecycle::Launch,
    }
}

#[test]
fn overlays_replace_routing_files_and_link_the_remaining_user_state() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".cline");
    fs::create_dir_all(source.join("data/settings")).expect("settings should exist");
    fs::create_dir_all(source.join("data/sessions")).expect("sessions should exist");
    fs::write(source.join("data/settings/providers.json"), "USER_PROVIDER")
        .expect("provider fixture should exist");
    fs::write(source.join("data/sessions/session.json"), "USER_SESSION")
        .expect("session fixture should exist");
    fs::write(source.join("hooks.json"), "USER_HOOKS").expect("hook fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "cline-config".to_owned(),
        path_hint: "cline".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.cline"),
        files: vec![OverlayFile {
            path: "data/settings/providers.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "NAN_PROVIDER".to_owned(),
            policy: OverlayFilePolicy::Replace,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("overlay should materialize");
    let overlay = workspace
        .path("cline-config")
        .expect("overlay should exist");

    assert_eq!(
        fs::read_to_string(overlay.join("data/settings/providers.json"))
            .expect("provider overlay should be readable"),
        "NAN_PROVIDER"
    );
    assert_eq!(
        fs::read_to_string(overlay.join("data/sessions/session.json"))
            .expect("linked session should be readable"),
        "USER_SESSION"
    );
    assert_eq!(
        fs::read_to_string(overlay.join("hooks.json")).expect("linked hook should be readable"),
        "USER_HOOKS"
    );
    #[cfg(unix)]
    {
        assert!(
            fs::symlink_metadata(overlay.join("data/sessions"))
                .expect("session link should have metadata")
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(overlay.join("hooks.json"))
                .expect("hook link should have metadata")
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn preserve_policy_creates_only_missing_fallback_files() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".openclaw");
    fs::create_dir_all(&source).expect("OpenClaw source should exist");
    fs::write(source.join("openclaw.json"), "USER_CONFIG").expect("OpenClaw fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "openclaw-config".to_owned(),
        path_hint: "openclaw".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.openclaw"),
        files: vec![
            OverlayFile {
                path: "openclaw.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "{}".to_owned(),
                policy: OverlayFilePolicy::Preserve,
            },
            OverlayFile {
                path: "nan-harness.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "{artifact:openclaw-config}/plugins".to_owned(),
                policy: OverlayFilePolicy::Replace,
            },
        ],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("overlay should materialize");
    let overlay = workspace
        .path("openclaw-config")
        .expect("overlay should exist");

    assert_eq!(
        fs::read_to_string(overlay.join("openclaw.json"))
            .expect("original config should remain readable"),
        "USER_CONFIG"
    );
    assert_eq!(
        fs::read_to_string(overlay.join("nan-harness.json"))
            .expect("nan-harness config should be readable"),
        format!("{}/plugins", overlay.display())
    );
}

#[test]
fn home_overlay_merges_routing_and_copies_mutable_state() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let storage = home.path().join(".agent-mock/global-storage");
    fs::create_dir_all(storage.join("tasks/session-1")).expect("agent state should exist");
    fs::write(
        storage.join("global-state.json"),
        r#"{"theme":"dark","nested":{"preserved":true}}"#,
    )
    .expect("agent state fixture should exist");
    fs::write(storage.join("secrets.json"), r#"{"userSecret":"keep"}"#)
        .expect("agent secrets fixture should exist");
    fs::write(storage.join("tasks/session-1/history.json"), "USER_SESSION")
        .expect("agent session fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "agent-home".to_owned(),
        path_hint: "agent-home".to_owned(),
        source_path: USER_HOME_PLACEHOLDER.to_owned(),
        files: vec![
            OverlayFile {
                path: ".agent-mock/global-storage/global-state.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template:
                    r#"{"openAiNativeBaseUrl":"http://127.0.0.1:1234/v1","nested":{"routing":true}}"#
                        .to_owned(),
                policy: OverlayFilePolicy::MergeJson,
            },
            OverlayFile {
                path: ".agent-mock/global-storage/secrets.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "{}".to_owned(),
                policy: OverlayFilePolicy::Copy,
            },
        ],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("agent home overlay should materialize");
    let overlay = workspace.path("agent-home").expect("overlay should exist");
    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(overlay.join(".agent-mock/global-storage/global-state.json"))
            .expect("merged state should be readable"),
    )
    .expect("merged state should be JSON");

    assert_eq!(state["theme"], "dark");
    assert_eq!(state["nested"]["preserved"], true);
    assert_eq!(state["nested"]["routing"], true);
    assert_eq!(state["openAiNativeBaseUrl"], "http://127.0.0.1:1234/v1");
    let overlay_secrets = overlay.join(".agent-mock/global-storage/secrets.json");
    assert_eq!(
        fs::read_to_string(&overlay_secrets).expect("copied secrets should be readable"),
        r#"{"userSecret":"keep"}"#
    );
    fs::write(&overlay_secrets, r#"{"bridgeToken":"temporary"}"#)
        .expect("temporary secrets should be writable");
    assert_eq!(
        fs::read_to_string(storage.join("secrets.json"))
            .expect("source secrets should remain readable"),
        r#"{"userSecret":"keep"}"#
    );
    assert_eq!(
        fs::read_to_string(
            overlay.join(".agent-mock/global-storage/tasks/session-1/history.json"),
        )
        .expect("linked agent session should be readable"),
        "USER_SESSION"
    );
}

#[test]
fn toml_overlay_merges_model_and_shares_codex_session_state() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".codex");
    fs::create_dir_all(&source).expect("Codex source should exist");
    fs::write(
        source.join("config.toml"),
        "model = \"qwen3.6\"\nmodel_provider = \"openai\"\n\n[profiles.default]\neffort = \"high\"\n",
    )
    .expect("Codex config fixture should exist");
    fs::write(source.join("state_5.sqlite"), [0, 1, 2, 3])
        .expect("Codex state fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "codex-home".to_owned(),
        path_hint: "codex-home".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
        files: vec![OverlayFile {
            path: "config.toml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "model = \"deepseek-v4-flash\"\n".to_owned(),
            policy: OverlayFilePolicy::MergeToml,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Codex overlay should materialize");
    let overlay = workspace.path("codex-home").expect("overlay should exist");
    let merged: toml::Table = toml::from_str(
        &fs::read_to_string(overlay.join("config.toml"))
            .expect("merged Codex config should be readable"),
    )
    .expect("merged Codex config should be TOML");

    assert_eq!(merged["model"].as_str(), Some("deepseek-v4-flash"));
    assert_eq!(merged["model_provider"].as_str(), Some("openai"));
    assert_eq!(
        merged["profiles"]["default"]["effort"].as_str(),
        Some("high")
    );
    assert!(
        fs::read_to_string(source.join("config.toml"))
            .expect("source Codex config should remain readable")
            .contains("model = \"qwen3.6\"")
    );
    let mirrored_state = overlay.join("state_5.sqlite");
    fs::write(&mirrored_state, [4, 5, 6, 7]).expect("mirrored state should be writable");
    assert_eq!(
        fs::read(source.join("state_5.sqlite")).expect("source state should be readable"),
        [4, 5, 6, 7]
    );
    #[cfg(unix)]
    assert!(
        fs::symlink_metadata(mirrored_state)
            .expect("mirrored state should have metadata")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn yaml_overlay_merges_maps_and_unions_plugin_lists() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".hermes");
    fs::create_dir_all(&source).expect("Hermes source should exist");
    fs::write(
        source.join("config.yaml"),
        "plugins:\n  enabled:\n    - user/plugin\n  disabled:\n    - blocked/plugin\nweb:\n  extract_backend: tavily\n",
    )
    .expect("Hermes config fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "hermes-home".to_owned(),
        path_hint: "hermes".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.hermes"),
        files: vec![OverlayFile {
            path: "config.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "plugins:\n  enabled:\n    - model-providers/nan\n    - web/nan\nweb:\n  search_backend: nan\n"
                .to_owned(),
            policy: OverlayFilePolicy::MergeYaml,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Hermes overlay should materialize");
    let merged: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        &fs::read_to_string(
            workspace
                .path("hermes-home")
                .expect("overlay should exist")
                .join("config.yaml"),
        )
        .expect("merged Hermes config should be readable"),
    )
    .expect("merged Hermes config should be YAML");
    let enabled = merged["plugins"]["enabled"]
        .as_sequence()
        .expect("enabled plugins should be a list");

    assert_eq!(enabled.len(), 3);
    assert!(enabled.contains(&serde_yaml_ng::Value::String("user/plugin".to_owned())));
    assert!(enabled.contains(&serde_yaml_ng::Value::String(
        "model-providers/nan".to_owned()
    )));
    assert!(enabled.contains(&serde_yaml_ng::Value::String("web/nan".to_owned())));
    assert_eq!(merged["plugins"]["disabled"][0], "blocked/plugin");
    assert_eq!(merged["web"]["extract_backend"], "tavily");
    assert_eq!(merged["web"]["search_backend"], "nan");
}

#[test]
fn toml_overlay_preserves_unmanaged_kimi_settings() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".kimi-code");
    fs::create_dir_all(&source).expect("Kimi Code source should exist");
    fs::write(
        source.join("config.toml"),
        "default_model = \"user/model\"\n\n[agents.review]\nprompt = \"Review carefully\"\n",
    )
    .expect("Kimi Code config fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "kimi-code-home".to_owned(),
        path_hint: "kimi-code".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.kimi-code"),
        files: vec![OverlayFile {
            path: "config.toml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "[models.\"nan/qwen3.6\"]\nmodel = \"qwen3.6\"\n".to_owned(),
            policy: OverlayFilePolicy::MergeToml,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Kimi Code overlay should materialize");
    let merged: toml::Table = toml::from_str(
        &fs::read_to_string(
            workspace
                .path("kimi-code-home")
                .expect("overlay should exist")
                .join("config.toml"),
        )
        .expect("merged Kimi Code config should be readable"),
    )
    .expect("merged Kimi Code config should be TOML");

    assert_eq!(merged["default_model"].as_str(), Some("user/model"));
    assert_eq!(
        merged["agents"]["review"]["prompt"].as_str(),
        Some("Review carefully")
    );
    assert_eq!(
        merged["models"]["nan/qwen3.6"]["model"].as_str(),
        Some("qwen3.6")
    );
}

#[test]
fn toml_overlay_relocates_codex_hook_state_to_the_mirrored_home() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".codex");
    fs::create_dir_all(&source).expect("Codex source should exist");
    fs::write(source.join("hooks.json"), "{\"hooks\":{}}").expect("Codex hooks should exist");
    fs::write(
        source.join("config.toml"),
        format!(
            "[hooks.state.\"{}:pre_tool_use:0:0\"]\ntrusted_hash = \"sha256:test\"\n",
            source.join("hooks.json").display()
        ),
    )
    .expect("Codex config should exist");
    let overlays = [ConfigurationOverlay {
        id: "codex-home".to_owned(),
        path_hint: "codex-home".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
        files: vec![OverlayFile {
            path: "config.toml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "model = \"deepseek-v4-flash\"\n".to_owned(),
            policy: OverlayFilePolicy::MergeToml,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Codex overlay should materialize");
    let overlay = workspace.path("codex-home").expect("overlay should exist");
    let merged: toml::Table = toml::from_str(
        &fs::read_to_string(overlay.join("config.toml"))
            .expect("merged Codex config should be readable"),
    )
    .expect("merged Codex config should be TOML");

    let state = merged["hooks"]["state"]
        .as_table()
        .expect("hook state should be a table");
    assert!(state.contains_key(&format!(
        "{}:pre_tool_use:0:0",
        overlay.join("hooks.json").display()
    )));
    let canonical_overlay = fs::canonicalize(overlay).expect("overlay should canonicalize");
    assert!(state.contains_key(&format!(
        "{}:pre_tool_use:0:0",
        canonical_overlay.join("hooks.json").display()
    )));
    assert!(state.contains_key(&format!(
        "{}:pre_tool_use:0:0",
        source.join("hooks.json").display()
    )));
}

#[test]
fn binary_copy_overlay_isolated_from_user_state() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".codex");
    fs::create_dir_all(&source).expect("Codex source should exist");
    fs::write(source.join("state_5.sqlite"), [0, 1, 2, 3])
        .expect("Codex state fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "codex-home".to_owned(),
        path_hint: "codex-home".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
        files: vec![OverlayFile {
            path: "state_5.sqlite".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: String::new(),
            policy: OverlayFilePolicy::CopyBinary,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Codex state overlay should materialize");
    let copied = workspace
        .path("codex-home")
        .expect("overlay should exist")
        .join("state_5.sqlite");
    assert_eq!(
        fs::read(&copied).expect("copied state should be readable"),
        [0, 1, 2, 3]
    );
    fs::write(&copied, [4, 5, 6, 7]).expect("copied state should be writable");
    assert_eq!(
        fs::read(source.join("state_5.sqlite")).expect("source state should be readable"),
        [0, 1, 2, 3]
    );
    #[cfg(unix)]
    assert!(
        !fs::symlink_metadata(copied)
            .expect("copied state should have metadata")
            .file_type()
            .is_symlink()
    );
}
