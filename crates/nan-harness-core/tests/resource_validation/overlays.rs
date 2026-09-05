use super::{assert_unsafe, base_plan, validate};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};

fn overlay() -> ConfigurationOverlay {
    ConfigurationOverlay {
        id: "settings-overlay".to_owned(),
        path_hint: "settings".to_owned(),
        source_path: USER_HOME_PLACEHOLDER.to_owned(),
        files: vec![OverlayFile {
            path: "config/settings.toml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "model = \"qwen3.6\"".to_owned(),
            policy: OverlayFilePolicy::Replace,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }
}

#[test]
fn overlays_accept_safe_home_paths_and_reject_unsafe_path_components() {
    let mut plan = base_plan();
    plan.configuration_overlays.push(overlay());
    validate(&plan).expect("safe overlay should be valid");

    for (path_hint, source_path, valid) in [
        (
            "nested/settings".to_owned(),
            USER_HOME_PLACEHOLDER.to_owned(),
            false,
        ),
        ("settings".to_owned(), "/tmp/user".to_owned(), false),
        (
            "settings".to_owned(),
            format!("{USER_HOME_PLACEHOLDER}/../other"),
            false,
        ),
        (
            "settings".to_owned(),
            format!("{USER_HOME_PLACEHOLDER}/nested/config"),
            true,
        ),
    ] {
        let mut plan = base_plan();
        let mut candidate = overlay();
        candidate.path_hint = path_hint;
        candidate.source_path = source_path;
        plan.configuration_overlays.push(candidate);
        if valid {
            validate(&plan).expect("nested user-home path should be valid");
        } else {
            assert_unsafe(&plan);
        }
    }
}

#[test]
fn overlay_ids_conflict_with_artifacts_and_each_other() {
    let mut plan = base_plan();
    let mut candidate = overlay();
    candidate.id = "opencode-config".to_owned();
    plan.configuration_overlays.push(candidate);
    assert_unsafe(&plan);

    let mut plan = base_plan();
    plan.configuration_overlays = vec![overlay(), overlay()];
    assert_unsafe(&plan);
}

#[test]
fn overlays_reject_contained_duplicate_and_insecure_files() {
    for files in [
        vec!["config/settings.toml", "config"],
        vec!["config/settings.toml", "config/settings.toml"],
        vec!["../settings.toml"],
        vec!["/tmp/settings.toml"],
    ] {
        let mut plan = base_plan();
        let mut candidate = overlay();
        candidate.files = files
            .into_iter()
            .map(|path| OverlayFile {
                path: path.to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: "content".to_owned(),
                policy: OverlayFilePolicy::Replace,
            })
            .collect();
        plan.configuration_overlays.push(candidate);
        assert_unsafe(&plan);
    }

    let mut plan = base_plan();
    let mut candidate = overlay();
    candidate.files[0].mode = TemporaryArtifactMode::OwnerDirectory;
    plan.configuration_overlays.push(candidate);
    assert_unsafe(&plan);
}

#[test]
fn overlay_templates_accept_artifact_text_and_reject_unknown_runtime_placeholders() {
    let mut plan = base_plan();
    let mut candidate = overlay();
    candidate.files[0].content_template = "{artifact:opencode-config}".to_owned();
    plan.configuration_overlays.push(candidate);
    validate(&plan).expect("known artifact in an overlay template should be valid");

    plan.configuration_overlays[0].files[0].content_template = "{runtime:missing}".to_owned();
    assert_unsafe(&plan);
}
