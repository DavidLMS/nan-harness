use super::super::super::TemporaryWorkspace;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use std::fs;

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
        .expect("overlay should materialize");
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
