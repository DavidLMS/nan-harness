use super::super::versioning::{
    CARGO_MANIFEST_FILES, LOCAL_PACKAGE_NAMES, replace_lockfile_version, replace_manifest_version,
};
use std::fs;

#[test]
fn version_updates_only_touch_workspace_and_local_packages() {
    assert!(CARGO_MANIFEST_FILES.contains(&"crates/nan-harness-private-fs/Cargo.toml"));
    assert!(LOCAL_PACKAGE_NAMES.contains(&"nan-harness-private-fs"));

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manifest = directory.path().join("Cargo.toml");
    fs::write(
        &manifest,
        concat!(
            "[workspace.package]\n",
            "version = \"0.0.1\"\n",
            "\n",
            "[workspace.dependencies]\n",
            "nan-harness-core = { path = \"core\", version = \"0.0.1\" }\n",
            "nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.1\" }\n",
            "nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.1\" }\n",
            "unrelated = { version = \"0.0.1\" }\n",
            "\n",
            "[dependencies.nan-harness-runtime]\n",
            "path = \"runtime\"\n",
            "version = \"0.0.1\"\n",
        ),
    )
    .expect("manifest fixture should exist");

    replace_manifest_version(&manifest, "0.0.1", "0.0.2").expect("manifest versions should update");
    let updated = fs::read_to_string(manifest).expect("updated manifest should be readable");

    assert!(updated.contains("version = \"0.0.2\""));
    assert!(updated.contains("nan-harness-core = { path = \"core\", version = \"0.0.2\" }"));
    assert!(
        updated
            .contains("nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.2\" }")
    );
    assert!(
        updated.contains(
            "[dependencies.nan-harness-runtime]\npath = \"runtime\"\nversion = \"0.0.2\""
        )
    );
    assert!(
        updated.contains("nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.2\" }")
    );
    assert!(updated.contains("unrelated = { version = \"0.0.1\" }"));

    let private_manifest = directory.path().join("private-fs/Cargo.toml");
    fs::create_dir_all(
        private_manifest
            .parent()
            .expect("fixture parent should exist"),
    )
    .expect("private filesystem fixture directory should exist");
    fs::write(
        &private_manifest,
        concat!(
            "[package]\n",
            "name = \"nan-harness-private-fs\"\n",
            "version = \"0.0.1\"\n",
            "\n",
            "[dev-dependencies]\n",
            "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.1\" }\n",
        ),
    )
    .expect("private filesystem manifest fixture should exist");

    replace_manifest_version(&private_manifest, "0.0.1", "0.0.2")
        .expect("private filesystem manifest versions should update");
    let private_updated =
        fs::read_to_string(&private_manifest).expect("private manifest should be readable");
    assert!(private_updated.contains("version = \"0.0.2\""));
    assert!(private_updated.contains(
        "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.2\" }"
    ));

    let lockfile = directory.path().join("Cargo.lock");
    fs::write(
        &lockfile,
        concat!(
            "version = 4\n\n",
            "[[package]]\n",
            "name = \"nan-harness-private-fs\"\n",
            "version = \"0.0.1\"\n",
            "dependencies = [\n",
            " \"nan-harness-test-support\",\n",
            "]\n\n",
            "[[package]]\n",
            "name = \"nan-harness-test-support\"\n",
            "version = \"0.0.1\"\n\n",
            "[[package]]\n",
            "name = \"unrelated\"\n",
            "version = \"0.0.1\"\n",
        ),
    )
    .expect("lockfile fixture should exist");

    replace_lockfile_version(&lockfile, "0.0.1", "0.0.2")
        .expect("local package lockfile versions should update");
    let lock_updated = fs::read_to_string(lockfile).expect("updated lockfile should be readable");
    assert!(lock_updated.contains("name = \"nan-harness-private-fs\"\nversion = \"0.0.2\""));
    assert!(lock_updated.contains("name = \"nan-harness-test-support\"\nversion = \"0.0.2\""));
    assert!(lock_updated.contains("name = \"unrelated\"\nversion = \"0.0.1\""));
}
