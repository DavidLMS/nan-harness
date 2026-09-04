use crate::temporary::TemporaryWorkspace;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use std::fs;

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
