use super::installation::parse_version_output;
use super::process::classify_early_exit;
use super::profile::{ManagedProfile, ProfileMarker, ensure_managed_profile};
use super::session::{SessionReceipt, desktop_config, restore_session};
use super::{
    CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME, PROFILE_MARKER_NAME,
    PROFILE_SCHEMA_VERSION, SESSION_RECEIPT_NAME, SESSION_SCHEMA_VERSION, SURFACE_ID,
};

fn profile(root: &std::path::Path) -> ManagedProfile {
    ManagedProfile {
        root: root.to_path_buf(),
        marker: root.join(PROFILE_MARKER_NAME),
        receipt: root.join(SESSION_RECEIPT_NAME),
        config: root.join(CONFIG_FILE_NAME),
        catalog: root.join(MODEL_CATALOG_FILE_NAME),
    }
}

#[test]
fn parses_app_and_codex_version_output() {
    assert_eq!(
        parse_version_output("26.825.51511\n")
            .expect("app version should parse")
            .to_string(),
        "26.825.51511"
    );
    assert_eq!(
        parse_version_output("codex-cli 0.151.0-alpha.7.2\n")
            .expect("Codex version should parse")
            .to_string(),
        "0.151.0-alpha.7.2"
    );
}

#[test]
fn recovery_removes_only_receipt_owned_session_files() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let profile = profile(&directory.path().join("profile"));
    ensure_managed_profile(&profile).expect("profile should be created");
    std::fs::write(&profile.config, "model = \"qwen3.6\"\n").expect("config should write");
    std::fs::write(&profile.catalog, "{}\n").expect("catalog should write");
    std::fs::write(
        &profile.receipt,
        serde_json::to_vec(&SessionReceipt {
            schema_version: SESSION_SCHEMA_VERSION,
            surface: SURFACE_ID.to_owned(),
            config_file: CONFIG_FILE_NAME.to_owned(),
            model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
        })
        .expect("receipt should serialize"),
    )
    .expect("receipt should write");
    std::fs::write(profile.root.join("auth.json"), "private\n")
        .expect("persistent state should write");

    assert!(restore_session(&profile).expect("recovery should succeed"));
    assert!(!profile.config.exists());
    assert!(!profile.catalog.exists());
    assert!(!profile.receipt.exists());
    assert!(profile.root.join("auth.json").exists());
}

#[test]
fn invalid_recovery_receipts_preserve_every_session_file() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let profile = profile(&directory.path().join("profile"));
    ensure_managed_profile(&profile).expect("profile should be created");
    std::fs::write(&profile.config, "model = \"qwen3.6\"\n").expect("config should write");
    std::fs::write(&profile.catalog, "{}\n").expect("catalog should write");
    std::fs::write(
        &profile.receipt,
        serde_json::to_vec(&SessionReceipt {
            schema_version: SESSION_SCHEMA_VERSION,
            surface: SURFACE_ID.to_owned(),
            config_file: "../config.toml".to_owned(),
            model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
        })
        .expect("receipt should serialize"),
    )
    .expect("receipt should write");

    assert!(matches!(
        restore_session(&profile),
        Err(ChatGptDesktopError::InvalidReceipt)
    ));
    assert!(profile.config.exists());
    assert!(profile.catalog.exists());
    assert!(profile.receipt.exists());
}

#[test]
fn desktop_config_contains_only_loopback_routing_and_a_token_reference() {
    let config = desktop_config(
        "qwen3.6",
        "http://127.0.0.1:43123",
        std::path::Path::new("/private/profile/nan-model-catalog.json"),
        true,
    )
    .expect("desktop config should render");
    let document = config
        .parse::<toml_edit::DocumentMut>()
        .expect("desktop config should be valid TOML");

    assert_eq!(document["model"].as_str(), Some("qwen3.6"));
    assert_eq!(
        document["model_providers"]["nan_harness"]["base_url"].as_str(),
        Some("http://127.0.0.1:43123/v1")
    );
    assert_eq!(
        document["model_providers"]["nan_harness"]["env_key"].as_str(),
        Some(super::SESSION_TOKEN_ENVIRONMENT)
    );
    assert_eq!(document["features"]["apps"].as_bool(), Some(false));
    assert!(!config.contains("NAN_API_KEY"));
    assert!(!config.contains("OPENAI_API_KEY"));
}

#[test]
fn invalid_profile_marker_is_rejected_without_claiming_the_directory() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let profile = profile(&directory.path().join("profile"));
    std::fs::create_dir_all(&profile.root).expect("profile should exist");
    std::fs::write(profile.root.join("user-owned"), "keep\n").expect("user file should write");

    assert!(matches!(
        ensure_managed_profile(&profile),
        Err(ChatGptDesktopError::UnmanagedProfile)
    ));
    assert!(profile.root.join("user-owned").exists());
}

#[test]
fn marker_contract_is_strict() {
    let marker = ProfileMarker {
        schema_version: PROFILE_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
    };
    let json = serde_json::to_value(marker).expect("marker should serialize");
    assert_eq!(json["schemaVersion"], PROFILE_SCHEMA_VERSION);
    assert_eq!(json["surface"], SURFACE_ID);
}

#[test]
fn early_exit_distinguishes_a_singleton_race_from_a_failed_start() {
    assert!(matches!(
        classify_early_exit(true, true),
        ChatGptDesktopError::SingletonRace
    ));
    assert!(matches!(
        classify_early_exit(true, false),
        ChatGptDesktopError::AppExitedDuringStartup
    ));
    assert!(matches!(
        classify_early_exit(false, true),
        ChatGptDesktopError::AppExitedDuringStartup
    ));
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::ChatGptDesktopError;
    use super::super::installation::discover_installation;
    use super::super::platform::is_chatgpt_app_root;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn script(path: &Path, output: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("script directory should exist");
        }
        fs::write(path, format!("#!/bin/sh\necho \"{output}\"\n")).expect("script should write");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("script should be executable");
    }

    fn fake_app_root(directory: &Path) {
        script(&directory.join("ChatGPT"), "ChatGPT 26.825.51511");
        script(
            &directory.join("resources/codex"),
            "codex-cli 0.151.0-alpha.7.2",
        );
    }

    fn canonical(path: &Path) -> std::path::PathBuf {
        fs::canonicalize(path).expect("path should exist")
    }

    #[test]
    fn discovery_resolves_an_explicit_app_root_and_reports_versions() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        fake_app_root(directory.path());
        let installation = discover_installation(Some(&directory.path().join("ChatGPT")))
            .expect("explicit executable should resolve");
        assert_eq!(
            fs::canonicalize(&installation.executable).expect("executable should exist"),
            canonical(&directory.path().join("ChatGPT"))
        );
        assert_eq!(installation.app_version.to_string(), "26.825.51511");
        assert_eq!(
            installation.bundled_codex_version.to_string(),
            "0.151.0-alpha.7.2"
        );
    }

    #[test]
    fn discovery_resolves_the_packaged_launcher_through_symlinks() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        fake_app_root(directory.path());
        let launcher = directory.path().join("chatgpt");
        std::os::unix::fs::symlink(directory.path().join("ChatGPT"), &launcher)
            .expect("launcher symlink should exist");
        let installation = discover_installation(Some(&launcher)).expect("launcher should resolve");
        assert_eq!(
            fs::canonicalize(&installation.executable).expect("executable should exist"),
            canonical(&directory.path().join("ChatGPT"))
        );
    }

    #[test]
    fn discovery_rejects_incomplete_installations() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        script(&directory.path().join("ChatGPT"), "ChatGPT 26.825.51511");
        assert!(matches!(
            discover_installation(Some(&directory.path().join("ChatGPT"))),
            Err(ChatGptDesktopError::InvalidInstallation)
        ));
    }

    #[test]
    fn discovery_reports_missing_installations() {
        assert!(matches!(
            discover_installation(Some(Path::new("/nonexistent/nan-harness-chatgpt"))),
            Err(ChatGptDesktopError::AppNotFound)
        ));
    }

    #[test]
    fn app_root_detection_requires_the_executable_and_bundled_codex() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        assert!(!is_chatgpt_app_root(directory.path()));
        fake_app_root(directory.path());
        assert!(is_chatgpt_app_root(directory.path()));
    }
}
