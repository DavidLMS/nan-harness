use super::{assert_unsafe, base_plan, validate};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CODEX_HOME_PLACEHOLDER, LaunchScopedFile, TemporaryArtifactMode,
};

fn scoped_file() -> LaunchScopedFile {
    LaunchScopedFile {
        id: "launch-config".to_owned(),
        directory: CODEX_HOME_PLACEHOLDER.to_owned(),
        file_name: "nan-harness-launch_01contract.config.toml".to_owned(),
        ownership_prefix: "nan-harness-launch_".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: "model = \"qwen3.6\"".to_owned(),
        lifecycle: ArtifactLifecycle::Launch,
    }
}

#[test]
fn launch_scoped_files_accept_runtime_home_and_owned_namespaces() {
    let mut plan = base_plan();
    plan.launch_scoped_files.push(scoped_file());
    validate(&plan).expect("owned launch-scoped file should be valid");
}

#[test]
fn launch_scoped_files_reject_unsafe_home_names_and_modes() {
    for (directory, file_name, prefix, mode) in [
        (
            "/tmp".to_owned(),
            "nan-harness-launch_a.toml".to_owned(),
            "nan-harness-launch_".to_owned(),
            TemporaryArtifactMode::OwnerFile,
        ),
        (
            CODEX_HOME_PLACEHOLDER.to_owned(),
            "../config.toml".to_owned(),
            "nan-harness-launch_".to_owned(),
            TemporaryArtifactMode::OwnerFile,
        ),
        (
            CODEX_HOME_PLACEHOLDER.to_owned(),
            "config.toml".to_owned(),
            "nan-harness-launch_".to_owned(),
            TemporaryArtifactMode::OwnerFile,
        ),
        (
            CODEX_HOME_PLACEHOLDER.to_owned(),
            "nan-harness-launch_a.toml".to_owned(),
            "other-".to_owned(),
            TemporaryArtifactMode::OwnerFile,
        ),
        (
            CODEX_HOME_PLACEHOLDER.to_owned(),
            "nan-harness-launch_a.toml".to_owned(),
            "nan-harness-launch_".to_owned(),
            TemporaryArtifactMode::OwnerDirectory,
        ),
    ] {
        let mut plan = base_plan();
        let mut file = scoped_file();
        file.directory = directory;
        file.file_name = file_name;
        file.ownership_prefix = prefix;
        file.mode = mode;
        plan.launch_scoped_files.push(file);
        assert_unsafe(&plan);
    }
}

#[test]
fn launch_scoped_file_paths_and_ids_are_unique_across_resources() {
    let mut plan = base_plan();
    plan.launch_scoped_files = vec![scoped_file(), scoped_file()];
    assert_unsafe(&plan);

    let mut plan = base_plan();
    let mut file = scoped_file();
    file.id = "opencode-config".to_owned();
    plan.launch_scoped_files.push(file);
    assert_unsafe(&plan);

    let mut plan = base_plan();
    let mut file = scoped_file();
    file.id = "second-launch-config".to_owned();
    plan.launch_scoped_files = vec![scoped_file(), file];
    assert_unsafe(&plan);
}
