use super::{SCOPED_FILES_LOCK_NAME, materialize_profile, session_lock_path};
use std::fs;

#[test]
fn failed_drop_coordination_leaves_recoverable_owned_files() {
    let home = tempfile::tempdir().expect("synthetic home");
    let workspace = materialize_profile(home.path(), "launch_01failedcleanup");
    let profile = workspace
        .path("codex-profile")
        .expect("profile")
        .to_path_buf();
    let session_lock = session_lock_path(&profile);
    let directory = profile.parent().expect("profile directory");
    let coordination = directory.join(SCOPED_FILES_LOCK_NAME);
    let base = directory.join("config.toml");
    fs::write(&base, "user-owned base").expect("base configuration");

    // Simulate an unavailable coordination entry after a successful publication.
    fs::remove_file(&coordination).expect("remove unlocked coordination fixture");
    fs::create_dir(&coordination).expect("block coordination with a nonregular entry");
    drop(workspace);
    assert_eq!(
        fs::read_to_string(&profile).expect("retained profile"),
        "model = \"qwen3.6\"\n"
    );
    assert!(session_lock.exists());

    fs::remove_dir(&coordination).expect("restore coordination availability");
    let recovery = materialize_profile(home.path(), "launch_01recoverfailedcleanup");
    assert!(!profile.exists());
    assert!(!session_lock.exists());
    assert_eq!(
        fs::read_to_string(&base).expect("preserved base"),
        "user-owned base"
    );
    assert_eq!(
        fs::read_to_string(recovery.path("codex-profile").expect("recovery profile"))
            .expect("recovery profile content"),
        "model = \"qwen3.6\"\n"
    );
    drop(recovery);
    assert!(coordination.is_file());
}
