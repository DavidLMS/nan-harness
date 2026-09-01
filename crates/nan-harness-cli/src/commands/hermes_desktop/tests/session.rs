use super::*;

#[test]
fn normal_restore_removes_only_the_launch_scoped_credential() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
        .expect("active parent");
    fs::write(paths.managed_profile.join(".env"), "USER_SETTING=before\n").expect("original env");
    fs::write(&paths.active_profile, b"{\"profile\":\"work\"}\n").expect("original active");

    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        "session-secret",
    )
    .expect("session setup");
    let active_env = fs::read_to_string(paths.managed_profile.join(".env")).expect("active env");
    assert!(active_env.contains("session-secret"));
    restore_session(&paths).expect("restore");

    assert_eq!(
        fs::read_to_string(paths.managed_profile.join(".env")).expect("restored env"),
        "USER_SETTING=before\n"
    );
    assert_eq!(
        fs::read(&paths.active_profile).expect("restored active"),
        b"{\"profile\":\"work\"}\n"
    );
    assert!(!paths.session_receipt.exists());
}

#[test]
fn restore_preserves_a_user_profile_switch() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        "session-secret",
    )
    .expect("session setup");
    fs::write(&paths.active_profile, b"{\"profile\":\"user-choice\"}\n").expect("user switch");

    restore_session(&paths).expect("restore");

    assert_eq!(
        fs::read(&paths.active_profile).expect("preserved active"),
        b"{\"profile\":\"user-choice\"}\n"
    );
    assert!(
        !fs::read_to_string(paths.managed_profile.join(".env"))
            .unwrap_or_default()
            .contains("session-secret")
    );
}

#[test]
fn receipt_never_contains_the_session_secret() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        "do-not-copy-this-secret",
    )
    .expect("session setup");

    let receipt = fs::read_to_string(&paths.session_receipt).expect("receipt");
    assert!(!receipt.contains("do-not-copy-this-secret"));
}

#[test]
fn restore_accepts_user_changes_after_the_session_credential_was_removed() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        "session-secret",
    )
    .expect("session setup");
    fs::write(paths.managed_profile.join(".env"), "USER_SETTING=changed\n")
        .expect("safe user edit");

    restore_session(&paths).expect("credential-free user edit is safe");

    assert_eq!(
        fs::read_to_string(paths.managed_profile.join(".env")).expect("preserved env"),
        "USER_SETTING=changed\n"
    );
    assert!(!paths.session_receipt.exists());
}

#[test]
fn stable_port_is_reused_from_owned_state() {
    let (_root, paths) = paths();
    let mut ownership = create_managed_profile(&paths).expect("managed profile");
    ownership.gateway_port = Some(43127);
    write_json_private(&paths.ownership_receipt, &ownership).expect("ownership update");

    let loaded = ensure_managed_profile(&paths).expect("owned profile");

    assert_eq!(loaded.gateway_port, Some(43127));
}

#[tokio::test]
async fn second_interrupt_during_update_preserves_recovery_state() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.hermes_home).expect("Hermes home");
    fs::write(
        &paths.update_marker,
        format!("{}\nstarted\n", std::process::id()),
    )
    .expect("live update marker");
    let (sender, mut signals) = tokio::sync::mpsc::unbounded_channel();
    sender.send(130).expect("first interrupt");
    sender.send(130).expect("second interrupt");
    let mut gateway = None;

    let result = wait_for_update(&paths, &mut gateway, &mut signals)
        .await
        .expect("update wait");

    assert_eq!(result, UpdateWaitCompletion::PreserveRecovery(130));
    assert!(paths.update_marker.exists());
}

#[test]
fn interrupt_protection_carries_across_update_and_relaunch() {
    let mut interrupt_seen = false;

    assert!(!update_interrupt_requests_exit(130, &mut interrupt_seen));
    assert!(interrupt_seen);
    assert!(update_interrupt_requests_exit(130, &mut interrupt_seen));
}

#[test]
fn desktop_relaunch_resets_the_termination_quiescence_window() {
    let start = Instant::now();
    let mut quiet_since = None;

    assert!(!desktop_quiescence_reached(
        &mut quiet_since,
        start,
        false,
        DESKTOP_QUIESCENCE_INTERVAL,
    ));
    assert!(!desktop_quiescence_reached(
        &mut quiet_since,
        start + Duration::from_secs(4),
        false,
        DESKTOP_QUIESCENCE_INTERVAL,
    ));
    assert!(!desktop_quiescence_reached(
        &mut quiet_since,
        start + Duration::from_secs(4),
        true,
        DESKTOP_QUIESCENCE_INTERVAL,
    ));
    assert!(!desktop_quiescence_reached(
        &mut quiet_since,
        start + Duration::from_secs(7),
        false,
        DESKTOP_QUIESCENCE_INTERVAL,
    ));
    assert!(desktop_quiescence_reached(
        &mut quiet_since,
        start + Duration::from_secs(12),
        false,
        DESKTOP_QUIESCENCE_INTERVAL,
    ));
}

#[test]
fn restore_is_idempotent_after_files_were_restored_before_receipt_cleanup() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    fs::write(paths.managed_profile.join(".env"), "USER_SETTING=before\n").expect("original env");
    begin_session(
        &paths,
        &paths.managed_profile,
        SessionMode::Persistent,
        "session-secret",
    )
    .expect("session setup");
    let receipt = read_optional_json::<SessionReceipt>(&paths.session_receipt)
        .expect("receipt read")
        .expect("receipt");
    restore_active_profile(&paths, &receipt).expect("active profile restore");
    restore_environment(&paths, &receipt).expect("environment restore");
    remove_if_exists(&paths.backup_directory.join("active-profile.backup"))
        .expect("active backup cleanup");
    remove_if_exists(&paths.backup_directory.join("profile-env.backup"))
        .expect("environment backup cleanup");

    restore_session(&paths).expect("repeated recovery should finish");

    assert!(!paths.session_receipt.exists());
    assert_eq!(
        fs::read_to_string(paths.managed_profile.join(".env")).expect("restored env"),
        "USER_SETTING=before\n"
    );
}
