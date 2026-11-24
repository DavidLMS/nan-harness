use crate::temporary::TemporaryWorkspace;
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

fn merge_with_policy(policy: OverlayFilePolicy, user_config: &str, patch: &str) -> toml::Table {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".vibe");
    fs::create_dir_all(&source).expect("Vibe source should exist");
    fs::write(source.join("config.toml"), user_config).expect("Vibe config fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "vibe-home".to_owned(),
        path_hint: "vibe-home".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.vibe"),
        files: vec![OverlayFile {
            path: "config.toml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: patch.to_owned(),
            policy,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];
    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Vibe overlay should materialize");
    toml::from_str(
        &fs::read_to_string(
            workspace
                .path("vibe-home")
                .expect("overlay should exist")
                .join("config.toml"),
        )
        .expect("merged Vibe config should be readable"),
    )
    .expect("merged Vibe config should be TOML")
}

const USER_VIBE_CONFIG: &str = r#"
active_model = "user"
textual_theme = "dark"

[[providers]]
name = "user-provider"
api_base = "https://example.invalid/v1"

[[models]]
name = "user-model"
alias = "user"
provider = "user-provider"

[[mcp_servers]]
name = "user-docs"
transport = "stdio"
command = "docs-server"

[[mcp_servers]]
name = "nan-search"
transport = "stdio"
command = "stale-search"
"#;

const NAN_VIBE_PATCH: &str = r#"
active_model = "qwen3.6"

[[providers]]
name = "nan"
api_base = "https://nan.invalid/v1"

[[models]]
name = "qwen3.6"
alias = "qwen3.6"
provider = "nan"

[[mcp_servers]]
name = "nan-search"
transport = "stdio"
command = "nan-harness"
"#;

#[test]
fn keyed_toml_overlay_keeps_user_mcp_servers_and_lets_the_patch_win_collisions() {
    let merged = merge_with_policy(
        OverlayFilePolicy::MergeTomlMcpServers,
        USER_VIBE_CONFIG,
        NAN_VIBE_PATCH,
    );

    let servers = merged["mcp_servers"].as_array().expect("mcp servers");
    let summary = servers
        .iter()
        .map(|server| {
            (
                server["name"].as_str().expect("name"),
                server["command"].as_str().expect("command"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [("user-docs", "docs-server"), ("nan-search", "nan-harness")]
    );
    // Only mcp_servers is keyed: routing tables are still replaced wholesale and
    // unrelated user settings survive.
    assert_eq!(merged["active_model"].as_str(), Some("qwen3.6"));
    assert_eq!(merged["textual_theme"].as_str(), Some("dark"));
    assert_eq!(merged["providers"].as_array().map(Vec::len), Some(1));
    assert_eq!(merged["providers"][0]["name"].as_str(), Some("nan"));
    assert_eq!(merged["models"].as_array().map(Vec::len), Some(1));
}

#[test]
fn keyed_toml_overlay_without_mcp_patch_or_user_servers_still_merges() {
    let without_patch_servers = merge_with_policy(
        OverlayFilePolicy::MergeTomlMcpServers,
        USER_VIBE_CONFIG,
        "active_model = \"qwen3.6\"\n",
    );
    assert_eq!(
        without_patch_servers["mcp_servers"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let without_user_servers = merge_with_policy(
        OverlayFilePolicy::MergeTomlMcpServers,
        "textual_theme = \"dark\"\n",
        NAN_VIBE_PATCH,
    );
    assert_eq!(
        without_user_servers["mcp_servers"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn keyed_toml_overlay_replaces_unkeyed_user_server_values_instead_of_guessing() {
    let merged = merge_with_policy(
        OverlayFilePolicy::MergeTomlMcpServers,
        "mcp_servers = [\"not-a-table\"]\n",
        NAN_VIBE_PATCH,
    );
    let servers = merged["mcp_servers"].as_array().expect("mcp servers");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["name"].as_str(), Some("nan-search"));
}

#[test]
fn plain_toml_overlay_still_replaces_arrays_wholesale() {
    let merged = merge_with_policy(
        OverlayFilePolicy::MergeToml,
        USER_VIBE_CONFIG,
        NAN_VIBE_PATCH,
    );
    let servers = merged["mcp_servers"].as_array().expect("mcp servers");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["command"].as_str(), Some("nan-harness"));
}
