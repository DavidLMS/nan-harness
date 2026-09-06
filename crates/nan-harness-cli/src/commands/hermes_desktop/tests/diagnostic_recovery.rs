use super::*;

const ORIGINAL_ACTIVE: &[u8] = b"{\"profile\":\"work\"}\n";
const SESSION_KEY: &str = "diagnostic-session-secret";

fn diagnostic_paths() -> (tempfile::TempDir, DesktopPaths) {
    let (root, paths) = paths();
    fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
        .expect("active parent");
    fs::write(&paths.active_profile, ORIGINAL_ACTIVE).expect("original active");
    (root, paths)
}

fn owner_marker(owner_id: &str) -> OwnerMarker {
    OwnerMarker {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: owner_id.to_owned(),
    }
}

/// Directory names directly under the profiles root, ordered so that the
/// filesystem's iteration order never becomes an assertion.
fn profile_names(paths: &DesktopPaths) -> Vec<String> {
    let mut names = fs::read_dir(&paths.profiles_root)
        .expect("profiles root")
        .map(|entry| {
            entry
                .expect("profile entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn a_diagnostic_session_removes_only_its_own_temporary_profile() {
    let (_root, paths) = diagnostic_paths();
    let profile = create_diagnostic_profile(&paths).expect("diagnostic profile");
    let neighbour = paths.profiles_root.join("work");
    fs::create_dir_all(&neighbour).expect("user profile");
    fs::write(neighbour.join("config.yaml"), "user: true\n").expect("user sentinel");

    begin_session(&paths, &profile, SessionMode::Diagnostic, SESSION_KEY).expect("session setup");
    assert!(profile.join(".env").exists());

    restore_session(&paths).expect("restore");

    assert_eq!(
        fs::read(&paths.active_profile).expect("restored active selection"),
        ORIGINAL_ACTIVE
    );
    assert!(!profile.exists());
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
    assert_eq!(
        fs::read_to_string(neighbour.join("config.yaml")).expect("user profile preserved"),
        "user: true\n"
    );
}

#[test]
fn a_tampered_diagnostic_marker_preserves_the_profile_and_allows_a_retry() {
    let (_root, paths) = diagnostic_paths();
    let profile = create_diagnostic_profile(&paths).expect("diagnostic profile");
    let marker = profile.join(OWNER_MARKER_FILE);
    let owned = fs::read(&marker).expect("owned marker");
    begin_session(&paths, &profile, SessionMode::Diagnostic, SESSION_KEY).expect("session setup");
    fs::write(profile.join("capture.log"), b"synthetic capture").expect("profile content");
    write_json_private(&marker, &owner_marker("someone-else")).expect("tampered marker");

    let error =
        restore_session(&paths).expect_err("an unowned profile must never be deleted by recovery");

    assert!(matches!(
        error,
        HermesDesktopError::DiagnosticOwnershipMismatch
    ));
    assert_eq!(
        fs::read(profile.join("capture.log")).expect("profile preserved"),
        b"synthetic capture"
    );
    assert!(paths.session_receipt.exists());
    // Only the profile removal is left for the retry: the active selection and
    // the temporary environment are restored before the ownership check.
    assert_eq!(
        fs::read(&paths.active_profile).expect("active selection"),
        ORIGINAL_ACTIVE
    );
    assert!(!profile.join(".env").exists());

    fs::write(&marker, &owned).expect("owned marker restored");
    restore_session(&paths).expect("recovery resumes once ownership is provable");

    assert!(!profile.exists());
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
}

#[test]
fn stale_cleanup_removes_only_correctly_owned_diagnostic_profiles() {
    let (_root, paths) = diagnostic_paths();
    let first = create_diagnostic_profile(&paths).expect("first diagnostic profile");
    let second = create_diagnostic_profile(&paths).expect("second diagnostic profile");
    let unowned = paths
        .profiles_root
        .join(format!("{DIAGNOSTIC_PROFILE_PREFIX}someone-else"));
    fs::create_dir_all(&unowned).expect("unowned prefixed profile");
    write_json_private(
        &unowned.join(OWNER_MARKER_FILE),
        &owner_marker("someone-else"),
    )
    .expect("unowned marker");
    fs::write(unowned.join("notes.txt"), b"unowned notes").expect("unowned sentinel");
    let unmarked = paths
        .profiles_root
        .join(format!("{DIAGNOSTIC_PROFILE_PREFIX}unmarked"));
    fs::create_dir_all(&unmarked).expect("unmarked prefixed profile");
    fs::write(unmarked.join("notes.txt"), b"unmarked notes").expect("unmarked sentinel");
    let unrelated = paths.profiles_root.join("work");
    fs::create_dir_all(&unrelated).expect("unrelated profile");
    fs::write(unrelated.join("config.yaml"), "user: true\n").expect("unrelated sentinel");
    let preserved = [
        "nan-diagnostic-someone-else",
        "nan-diagnostic-unmarked",
        "work",
    ];

    cleanup_stale_diagnostic_profiles(&paths).expect("stale cleanup");

    assert!(!first.exists());
    assert!(!second.exists());
    assert_eq!(profile_names(&paths), preserved);
    assert_eq!(
        fs::read(unowned.join("notes.txt")).expect("unowned notes preserved"),
        b"unowned notes"
    );
    assert_eq!(
        fs::read(unmarked.join("notes.txt")).expect("unmarked notes preserved"),
        b"unmarked notes"
    );
    assert_eq!(
        fs::read_to_string(unrelated.join("config.yaml")).expect("unrelated profile preserved"),
        "user: true\n"
    );

    cleanup_stale_diagnostic_profiles(&paths).expect("a repeated cleanup is harmless");

    assert_eq!(profile_names(&paths), preserved);
}
