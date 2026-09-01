use super::*;

#[test]
fn profile_config_preserves_unrelated_settings_and_provider_entries() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile directory");
    fs::write(
        paths.managed_profile.join("config.yaml"),
        "theme: dark\nmodel:\n  default: old\n  context_length: 12\n# keep this provider comment\nproviders:\n  other:\n    base_url: https://example.test/v1\n  nan:\n    base_url: http://old.test/v1\n  inline: {base_url: https://inline.example.test/v1}\ntools:\n  enabled: true\n",
    )
    .expect("original config");
    let models = vec![CodingModelProfile::generic("qwen3.6")];

    write_profile_config(
        &paths.managed_profile,
        "http://127.0.0.1:4321/v1",
        &models,
        "qwen3.6",
        false,
    )
    .expect("config update");

    let updated =
        fs::read_to_string(paths.managed_profile.join("config.yaml")).expect("updated config");
    assert!(updated.contains("theme: dark"));
    assert!(updated.contains("tools:\n  enabled: true"));
    assert!(updated.contains("# keep this provider comment"));
    assert!(updated.contains("  other:\n    base_url: https://example.test/v1"));
    assert!(updated.contains("  inline: {base_url: https://inline.example.test/v1}"));
    assert!(updated.contains("base_url: \"http://127.0.0.1:4321/v1\""));
    assert!(!updated.contains("http://old.test"));
}

#[test]
fn profile_search_reuses_the_adapter_renderer_and_disables_only_owned_settings() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile directory");
    fs::write(
        paths.managed_profile.join("config.yaml"),
        "theme: dark\nweb:\n  user_setting: kept\n",
    )
    .expect("original config");
    let models = vec![CodingModelProfile::generic("qwen3.6")];

    write_profile_config(
        &paths.managed_profile,
        "http://127.0.0.1:4321/v1",
        &models,
        "qwen3.6",
        true,
    )
    .expect("search-enabled config update");

    let provider = fs::read_to_string(
        paths
            .managed_profile
            .join("plugins/web/nan_harness/provider.py"),
    )
    .expect("shared provider renderer should be installed");
    assert!(provider.contains("http://127.0.0.1:4321/v1/search"));
    let enabled: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        &fs::read_to_string(paths.managed_profile.join("config.yaml")).expect("enabled config"),
    )
    .expect("enabled YAML");
    assert_eq!(enabled["web"]["search_backend"], "nan-harness");
    assert_eq!(enabled["web"]["user_setting"], "kept");

    write_profile_config(
        &paths.managed_profile,
        "http://127.0.0.1:4321/v1",
        &models,
        "qwen3.6",
        false,
    )
    .expect("search-disabled config update");
    let disabled: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        &fs::read_to_string(paths.managed_profile.join("config.yaml")).expect("disabled config"),
    )
    .expect("disabled YAML");
    assert!(disabled["web"].get("search_backend").is_none());
    assert_eq!(disabled["web"]["user_setting"], "kept");
}

#[test]
fn unmanaged_nan_profile_is_never_adopted() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("existing profile");
    fs::write(paths.managed_profile.join("config.yaml"), "user: true\n").expect("user config");

    let error = ensure_managed_profile(&paths).expect_err("profile should conflict");

    assert!(matches!(error, HermesDesktopError::UnmanagedNanProfile));
    assert_eq!(
        fs::read_to_string(paths.managed_profile.join("config.yaml"))
            .expect("user config preserved"),
        "user: true\n"
    );
}

#[test]
fn locate_managed_profile_accepts_an_active_profile() {
    let (_root, paths) = paths();
    let ownership = create_managed_profile(&paths).expect("managed profile");
    activate_managed_profile(&paths, &ownership).expect("active profile");

    assert_eq!(
        locate_managed_profile(&paths).expect("locate active profile"),
        Some((ownership, ManagedProfileLocation::Active))
    );
}

#[test]
fn receipt_without_a_profile_is_reported_as_missing() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("managed profile");
    fs::remove_dir_all(&paths.parked_profile).expect("remove parked profile");
    fs::remove_file(&paths.managed_profile).expect("remove visibility guard");

    assert!(matches!(
        locate_managed_profile(&paths),
        Err(HermesDesktopError::ManagedProfileMissing)
    ));
}

#[test]
fn incompatible_owner_marker_is_not_adopted() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("managed profile");
    fs::write(
        paths.parked_profile.join(OWNER_MARKER_FILE),
        br#"{"schemaVersion":1,"ownerId":"different-owner"}"#,
    )
    .expect("incompatible marker");

    assert!(matches!(
        locate_managed_profile(&paths),
        Err(HermesDesktopError::OwnershipMismatch)
    ));
}

#[test]
fn owned_profile_survives_nan_harness_state_removal() {
    let (_root, paths) = paths();
    let original = create_managed_profile(&paths).expect("managed profile");
    fs::remove_file(&paths.ownership_receipt).expect("simulate removed application state");

    let recovered = ensure_managed_profile(&paths).expect("owned marker should be recoverable");

    assert_eq!(recovered.owner_id, original.owner_id);
    assert_eq!(recovered.gateway_port, None);
    assert_eq!(
        profile_path_kind(&paths.managed_profile).expect("guard kind"),
        ProfilePathKind::RegularFile
    );
    assert!(paths.parked_profile.exists());
}

#[test]
fn managed_profile_is_parked_between_sessions_without_losing_state() {
    let (_root, paths) = paths();
    let ownership = create_managed_profile(&paths).expect("managed profile");
    fs::write(paths.parked_profile.join("state.db"), b"persistent state")
        .expect("persistent state");
    fs::create_dir(paths.parked_profile.join("sessions")).expect("sessions directory");
    fs::write(
        paths.parked_profile.join("sessions/conversation.json"),
        b"persistent session",
    )
    .expect("persistent session");

    activate_managed_profile(&paths, &ownership).expect("activate profile");

    assert!(paths.managed_profile.exists());
    assert!(!paths.parked_profile.exists());
    assert_eq!(
        fs::read(paths.managed_profile.join("state.db")).expect("active state"),
        b"persistent state"
    );
    fs::create_dir_all(paths.active_profile.parent().expect("active parent"))
        .expect("active parent");
    fs::write(&paths.active_profile, b"{\"profile\":\"nan\"}\n").expect("managed active selection");

    park_managed_profile(&paths).expect("park profile");

    assert_eq!(
        profile_path_kind(&paths.managed_profile).expect("guard kind"),
        ProfilePathKind::RegularFile
    );
    assert!(paths.parked_profile.exists());
    assert_eq!(
        fs::read(paths.parked_profile.join("state.db")).expect("parked state"),
        b"persistent state"
    );
    assert_eq!(
        fs::read(paths.parked_profile.join("sessions/conversation.json")).expect("parked session"),
        b"persistent session"
    );
    assert_eq!(
        read_optional_json::<serde_json::Value>(&paths.active_profile)
            .expect("active selection read")
            .expect("active selection"),
        json!({"profile": "default"})
    );

    activate_managed_profile(&paths, &ownership).expect("reactivate profile");
    fs::write(&paths.active_profile, b"{\"profile\":\"work\"}\n").expect("user active selection");
    park_managed_profile(&paths).expect("repark profile");

    assert_eq!(
        fs::read(&paths.active_profile).expect("user selection preserved"),
        b"{\"profile\":\"work\"}\n"
    );
}

#[test]
fn duplicate_active_and_parked_profiles_are_left_untouched() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("parked managed profile");
    fs::remove_file(&paths.managed_profile).expect("remove visibility guard");
    fs::create_dir(&paths.managed_profile).expect("duplicate active profile");
    fs::write(paths.managed_profile.join("active.txt"), b"active").expect("active sentinel");
    fs::write(paths.parked_profile.join("parked.txt"), b"parked").expect("parked sentinel");

    let error = ensure_managed_profile(&paths).expect_err("duplicate should conflict");

    assert!(matches!(error, HermesDesktopError::ManagedProfileConflict));
    assert_eq!(
        fs::read(paths.managed_profile.join("active.txt")).expect("active preserved"),
        b"active"
    );
    assert_eq!(
        fs::read(paths.parked_profile.join("parked.txt")).expect("parked preserved"),
        b"parked"
    );
}

#[test]
fn visibility_guard_blocks_cached_desktop_profile_recreation() {
    let (_root, paths) = paths();
    let ownership = create_managed_profile(&paths).expect("parked managed profile");

    let guard = read_optional_json::<OwnerMarker>(&paths.managed_profile)
        .expect("guard read")
        .expect("guard");
    let recreate = fs::create_dir_all(&paths.managed_profile)
        .expect_err("a cached Desktop backend must not recreate the profile");

    assert_eq!(recreate.kind(), ErrorKind::AlreadyExists);
    assert_eq!(guard.owner_id, ownership.owner_id);
    assert!(paths.parked_profile.exists());
}

#[test]
fn restore_quarantines_an_empty_recreated_profile_without_deleting_it() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("parked managed profile");
    fs::remove_file(&paths.managed_profile).expect("simulate legacy parking");
    fs::create_dir(&paths.managed_profile).expect("recreated profile");
    fs::write(paths.managed_profile.join("state.db"), b"recreated state").expect("recreated state");

    quarantine_recreated_profile_for_restore(&paths).expect("quarantine recreated profile");

    assert_eq!(
        profile_path_kind(&paths.managed_profile).expect("guard kind"),
        ProfilePathKind::RegularFile
    );
    assert!(paths.parked_profile.exists());
    let recovered = fs::read_dir(&paths.recovered_profiles_root)
        .expect("recovery directory")
        .map(|entry| entry.expect("recovery entry").path())
        .collect::<Vec<_>>();
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        fs::read(recovered[0].join("state.db")).expect("recreated state preserved"),
        b"recreated state"
    );
}

#[test]
fn restore_does_not_quarantine_a_configured_duplicate_profile() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("parked managed profile");
    fs::remove_file(&paths.managed_profile).expect("remove visibility guard");
    fs::create_dir(&paths.managed_profile).expect("user profile");
    fs::write(paths.managed_profile.join("config.yaml"), "user: true\n").expect("user config");

    quarantine_recreated_profile_for_restore(&paths).expect("safe recovery check");

    assert!(paths.managed_profile.is_dir());
    assert!(!paths.recovered_profiles_root.exists());
}

#[test]
fn tampered_visibility_guard_is_never_replaced() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("parked managed profile");
    fs::write(&paths.managed_profile, b"not the owned guard").expect("tamper visibility guard");

    let error = ensure_managed_profile(&paths).expect_err("tampered guard should conflict");

    assert!(matches!(
        error,
        HermesDesktopError::ProfileGuardOwnershipMismatch
    ));
    assert_eq!(
        fs::read(&paths.managed_profile).expect("guard preserved"),
        b"not the owned guard"
    );
}

#[test]
fn unowned_parked_profile_is_never_adopted() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.parked_profile).expect("existing parked profile");
    fs::write(paths.parked_profile.join("config.yaml"), "user: true\n").expect("user config");

    let error = ensure_managed_profile(&paths).expect_err("profile should conflict");

    assert!(matches!(
        error,
        HermesDesktopError::ParkedProfileOwnershipMismatch
    ));
    assert_eq!(
        fs::read_to_string(paths.parked_profile.join("config.yaml"))
            .expect("user config preserved"),
        "user: true\n"
    );
}

#[test]
fn legacy_display_name_is_removed_only_when_unmodified() {
    let (_root, paths) = paths();
    fs::create_dir_all(&paths.managed_profile).expect("profile");
    let metadata = paths.managed_profile.join("profile.yaml");
    fs::write(&metadata, "display_name: NaN\n").expect("legacy metadata");

    remove_legacy_profile_display_name(&paths.managed_profile).expect("remove legacy metadata");

    assert!(!metadata.exists());

    fs::write(&metadata, "display_name: NaN\ndescription: keep\n").expect("custom metadata");
    remove_legacy_profile_display_name(&paths.managed_profile)
        .expect("preserve customized metadata");

    assert_eq!(
        fs::read_to_string(&metadata).expect("custom metadata preserved"),
        "display_name: NaN\ndescription: keep\n"
    );
}
