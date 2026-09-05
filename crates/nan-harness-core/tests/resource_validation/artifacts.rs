use super::{assert_invalid, assert_unsafe, base_plan, validate};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode,
};

#[test]
fn artifact_ids_accept_the_documented_length_and_charset_boundaries() {
    for id in [
        "abc".to_owned(),
        format!("a{}", "b".repeat(63)),
        "a0_b-c".to_owned(),
    ] {
        let mut plan = base_plan();
        plan.temporary_artifacts[0].id = id;
        validate(&plan).expect("boundary artifact ID should be valid");
    }

    for id in [
        "ab".to_owned(),
        format!("a{}", "b".repeat(64)),
        "Avalid-id".to_owned(),
        "ab.c".to_owned(),
        "a b".to_owned(),
    ] {
        let mut plan = base_plan();
        plan.temporary_artifacts[0].id = id;
        assert_unsafe(&plan);
    }
}

#[test]
fn artifact_ids_are_unique_across_all_temporary_resource_kinds() {
    let mut plan = base_plan();
    plan.temporary_artifacts
        .push(plan.temporary_artifacts[0].clone());
    assert_unsafe(&plan);
}

#[test]
fn artifacts_require_matching_kind_mode_and_content() {
    let mut plan = base_plan();
    plan.temporary_artifacts[0].kind = TemporaryArtifactKind::Directory;
    plan.temporary_artifacts[0].mode = TemporaryArtifactMode::OwnerDirectory;
    plan.temporary_artifacts[0].content_template = None;
    validate(&plan).expect("owner directory without content should be valid");

    for (kind, mode, content) in [
        (
            TemporaryArtifactKind::File,
            TemporaryArtifactMode::OwnerDirectory,
            Some("x"),
        ),
        (
            TemporaryArtifactKind::File,
            TemporaryArtifactMode::OwnerFile,
            None,
        ),
        (
            TemporaryArtifactKind::Directory,
            TemporaryArtifactMode::OwnerFile,
            None,
        ),
        (
            TemporaryArtifactKind::Directory,
            TemporaryArtifactMode::OwnerDirectory,
            Some("x"),
        ),
    ] {
        let mut plan = base_plan();
        plan.temporary_artifacts[0].kind = kind;
        plan.temporary_artifacts[0].mode = mode;
        plan.temporary_artifacts[0].content_template = content.map(str::to_owned);
        assert_unsafe(&plan);
    }
}

#[test]
fn artifact_path_hints_are_single_safe_components() {
    for path_hint in [
        "config.toml",
        "nested/config.toml",
        "../config.toml",
        "/tmp/config.toml",
        "",
    ] {
        let mut plan = base_plan();
        plan.temporary_artifacts[0].path_hint = path_hint.to_owned();
        if path_hint == "config.toml" {
            validate(&plan).expect("single normal component should be valid");
        } else {
            assert_unsafe(&plan);
        }
    }
}

#[test]
fn artifact_references_can_contain_multiple_resources() {
    let mut plan = base_plan();
    plan.temporary_artifacts.push(TemporaryArtifact {
        id: "second-artifact".to_owned(),
        kind: TemporaryArtifactKind::File,
        path_hint: "second.txt".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: Some("second".to_owned()),
        lifecycle: ArtifactLifecycle::Launch,
    });
    plan.process.arguments =
        vec!["{artifact:opencode-config}/{artifact:second-artifact}".to_owned()];
    validate(&plan).expect("all referenced artifacts should exist");

    plan.process.arguments[0].push_str("/{artifact:missing}");
    assert_invalid(&plan, "process.arguments");
}
