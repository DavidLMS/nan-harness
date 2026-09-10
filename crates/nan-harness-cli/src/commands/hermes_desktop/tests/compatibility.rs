use super::*;
use crate::app::{HarnessRunArgs, WebSearchArgs};

fn hermes_arguments() -> HermesDesktopArgs {
    HermesDesktopArgs {
        run: HarnessRunArgs {
            model: None,
            executable: None,
            provider_base_url: None,
            allow_unsupported: false,
            allow_untested: false,
            search: WebSearchArgs::default(),
            dry_run: false,
            session_max_tokens: None,
            context: None,
            arguments: Vec::new(),
        },
        no_chat_gateway: false,
        restore: false,
    }
}

fn assert_restore_conflict(set_launch_option: impl FnOnce(&mut HermesDesktopArgs)) {
    let mut arguments = hermes_arguments();
    arguments.restore = true;
    set_launch_option(&mut arguments);
    assert!(matches!(
        validate_arguments(&arguments),
        Err(HermesDesktopError::RestoreWithLaunchOptions)
    ));
}

#[test]
fn removing_an_absent_profile_does_not_inspect_host_processes() {
    let (_root, paths) = paths();

    let removed = remove_persistent_profile_at(&paths, || {
        panic!("the process table must not be inspected without managed Hermes state")
    })
    .expect("an absent profile should be a no-op");

    assert!(!removed);
    assert!(!paths.state_directory.exists());
    assert!(!paths.hermes_home.exists());
}

#[test]
fn a_running_desktop_preserves_an_owned_profile() {
    let (_root, paths) = paths();
    create_managed_profile(&paths).expect("managed profile");
    let receipt = fs::read(&paths.ownership_receipt).expect("ownership receipt");
    let marker =
        fs::read(paths.parked_profile.join(OWNER_MARKER_FILE)).expect("profile ownership marker");

    let error = remove_persistent_profile_at(&paths, || {
        Ok(Some(DesktopProcess {
            pid: 42,
            started: "test process".to_owned(),
        }))
    })
    .expect_err("a running desktop should block profile removal");

    assert!(matches!(error, HermesDesktopError::AlreadyRunning));
    assert_eq!(
        fs::read(&paths.ownership_receipt).expect("preserved ownership receipt"),
        receipt
    );
    assert_eq!(
        fs::read(paths.parked_profile.join(OWNER_MARKER_FILE))
            .expect("preserved profile ownership marker"),
        marker
    );
}

#[test]
fn desktop_version_requires_0206_unless_overridden() {
    assert!(validate_desktop_version("Hermes 0.20.6", false, false).is_ok());
    assert!(validate_desktop_version("hermes 0.21.0+desktop", false, false).is_ok());
    assert!(matches!(
        validate_desktop_version("Hermes 0.20.5", false, false),
        Err(HermesDesktopError::DesktopVersionUnsupported { detected, minimum })
            if detected == Version::new(0, 20, 5) && minimum == Version::new(0, 20, 6)
    ));
    assert!(validate_desktop_version("Hermes 0.20.5", true, false).is_ok());
}

#[test]
fn restore_rejects_each_launch_field_independently() {
    assert_restore_conflict(|arguments| arguments.run.model = Some("model".to_owned()));
    assert_restore_conflict(|arguments| {
        arguments.run.executable = Some(PathBuf::from("/tmp/hermes"));
    });
    assert_restore_conflict(|arguments| {
        arguments.run.provider_base_url = Some("https://provider.invalid/v1".to_owned());
    });
    assert_restore_conflict(|arguments| arguments.run.allow_unsupported = true);
    assert_restore_conflict(|arguments| arguments.run.allow_untested = true);
    assert_restore_conflict(|arguments| arguments.run.dry_run = true);
    assert_restore_conflict(|arguments| arguments.run.session_max_tokens = Some(100));
    assert_restore_conflict(|arguments| arguments.run.context = Some(100));
    assert_restore_conflict(|arguments| arguments.no_chat_gateway = true);
    assert_restore_conflict(|arguments| arguments.run.arguments.push("--help".to_owned()));
}

#[test]
fn restore_alone_is_valid_and_harmless_launch_options_are_valid_without_restore() {
    let mut restore = hermes_arguments();
    restore.restore = true;
    assert!(validate_arguments(&restore).is_ok());

    let mut launch = hermes_arguments();
    launch.run.model = Some("model".to_owned());
    launch.run.executable = Some(PathBuf::from("/tmp/hermes"));
    launch.run.provider_base_url = Some("https://provider.invalid/v1".to_owned());
    launch.run.allow_unsupported = true;
    launch.run.allow_untested = true;
    launch.run.dry_run = true;
    launch.run.session_max_tokens = Some(100);
    launch.run.context = Some(100);
    launch.no_chat_gateway = true;
    launch.run.arguments.push("--help".to_owned());
    assert!(validate_arguments(&launch).is_ok());
}

#[test]
fn desktop_capability_probe_requires_managed_launch_options() {
    assert!(missing_desktop_capabilities("--source --skip-build --cwd").is_empty());
    assert_eq!(
        missing_desktop_capabilities("--source --cwd"),
        vec!["--skip-build"]
    );
}

#[test]
fn managed_launch_rejects_native_one_shot_desktop_options() {
    for (argument, expected) in [
        ("--build-only", "--build-only"),
        ("--setup-tcc-identity", "--setup-tcc-identity"),
    ] {
        let mut arguments = hermes_arguments();
        arguments.run.arguments.push(argument.to_owned());
        assert!(matches!(
            validate_arguments(&arguments),
            Err(HermesDesktopError::UnsupportedDesktopArgument(value)) if value == expected
        ));
    }

    let mut harmless = hermes_arguments();
    harmless.run.arguments.push("--source".to_owned());
    assert!(validate_arguments(&harmless).is_ok());
}

#[test]
fn model_selection_prefers_requested_then_default_then_first_catalog_entry() {
    let models = vec![
        CodingModelProfile::generic("first"),
        CodingModelProfile::generic(DEFAULT_MODEL_ID),
        CodingModelProfile::generic("requested"),
    ];

    assert_eq!(
        select_model(&models, Some("requested")).expect("requested model"),
        "requested"
    );
    assert_eq!(
        select_model(&models, None).expect("default model"),
        DEFAULT_MODEL_ID
    );

    let without_default = vec![
        CodingModelProfile::generic("first"),
        CodingModelProfile::generic("second"),
    ];
    assert_eq!(
        select_model(&without_default, None).expect("first catalog model"),
        "first"
    );
}

#[test]
fn model_selection_reports_unavailable_requests_and_empty_catalogs() {
    let models = vec![
        CodingModelProfile::generic("first"),
        CodingModelProfile::generic("second"),
    ];
    assert!(matches!(
        select_model(&models, Some("missing")),
        Err(HermesDesktopError::ModelUnavailable { model, available })
            if model == "missing" && available == ["first", "second"]
    ));
    assert!(matches!(
        select_model(&[], Some("missing")),
        Err(HermesDesktopError::ModelUnavailable { model, available })
            if model == "missing" && available.is_empty()
    ));
    assert!(matches!(
        select_model(&[], None),
        Err(HermesDesktopError::EmptyModelCatalog)
    ));
}

#[test]
fn alternate_hermes_root_disables_automatic_skip_build() {
    let (root, paths) = paths();
    fs::create_dir_all(paths.install_root.join("apps/desktop/release/mac-arm64"))
        .expect("macOS release directory");
    let packaged = packaged_desktop_candidates(&paths.install_root)
        .into_iter()
        .next()
        .expect("packaged desktop candidate");
    fs::create_dir_all(packaged.parent().expect("packaged desktop parent"))
        .expect("packaged desktop parent");
    fs::write(&packaged, b"desktop").expect("packaged desktop");

    assert_eq!(
        desktop_arguments(&paths, &[]),
        vec!["desktop", "--skip-build"]
    );
    assert_eq!(
        desktop_arguments(
            &paths,
            &[
                "--hermes-root".to_owned(),
                root.path().display().to_string()
            ]
        ),
        vec![
            "desktop".to_owned(),
            "--hermes-root".to_owned(),
            root.path().display().to_string()
        ]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_packaged_candidates_cover_both_architectures_and_binary_names() {
    let root = Path::new("/opt/hermes");

    assert_eq!(
        packaged_desktop_candidates(root),
        [
            "apps/desktop/release/linux-unpacked/hermes",
            "apps/desktop/release/linux-unpacked/Hermes",
            "apps/desktop/release/linux-arm64-unpacked/hermes",
            "apps/desktop/release/linux-arm64-unpacked/Hermes",
        ]
        .map(|candidate| root.join(candidate))
    );
}

#[cfg(unix)]
#[test]
fn unix_process_classification_ignores_electron_helpers() {
    assert!(desktop_main_command(
        "/opt/hermes/apps/desktop/release/linux-unpacked/Hermes"
    ));
    assert!(desktop_main_command(
        "/opt/hermes/apps/desktop/node_modules/electron/dist/electron /opt/hermes/apps/desktop"
    ));
    assert!(!desktop_main_command(
        "/opt/hermes/apps/desktop/release/linux-unpacked/Hermes --type=renderer"
    ));
}

#[test]
fn windows_process_classification_ignores_electron_helpers() {
    assert!(windows_desktop_main_process(
        "Hermes.exe",
        r"C:\Hermes\Hermes.exe"
    ));
    assert!(windows_desktop_main_process(
        "electron.exe",
        r"C:\repo\apps\desktop\node_modules\electron\electron.exe C:\repo\apps\desktop"
    ));
    assert!(!windows_desktop_main_process(
        "Hermes.exe",
        r"C:\Hermes\Hermes.exe --type=renderer"
    ));
}

#[test]
fn windows_process_listing_selects_only_the_main_process() {
    let listing = r#"[
        {"ProcessId": 41, "CreationDate": "renderer", "Name": "Hermes.exe", "CommandLine": "Hermes.exe --type=renderer"},
        {"ProcessId": 42, "CreationDate": "main", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"}
    ]"#;

    assert_eq!(
        parse_windows_process_listing(listing).expect("valid process listing"),
        Some(DesktopProcess {
            pid: 42,
            started: "main".to_owned()
        })
    );
}

#[test]
fn windows_process_listing_fails_closed_when_multiple_mains_exist() {
    let listing = r#"[
        {"ProcessId": 42, "CreationDate": "one", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"},
        {"ProcessId": 43, "CreationDate": "two", "Name": "Hermes.exe", "CommandLine": "Hermes.exe"}
    ]"#;

    assert!(matches!(
        parse_windows_process_listing(listing),
        Err(HermesDesktopError::AmbiguousDesktopProcesses)
    ));
}

#[test]
fn windows_hermes_home_prefers_user_scope_then_modern_then_legacy() {
    let root = tempfile::tempdir().expect("temporary root");
    let home = root.path().join("user");
    let local_app_data = home.join("AppData/Local");
    let modern = local_app_data.join("hermes");
    let legacy = home.join(".hermes");
    let user_scoped = root.path().join("custom-hermes");

    assert_eq!(
        choose_windows_hermes_home(
            &home,
            Some(user_scoped.clone()),
            Some(local_app_data.clone())
        ),
        user_scoped
    );
    fs::create_dir_all(&legacy).expect("legacy Hermes home");
    assert_eq!(
        choose_windows_hermes_home(&home, None, Some(local_app_data.clone())),
        legacy
    );
    fs::create_dir_all(&modern).expect("modern Hermes home");
    assert_eq!(
        choose_windows_hermes_home(&home, None, Some(local_app_data)),
        modern
    );
}
