use super::{
    CodingModelProfile, DesktopPaths, HermesDesktopError, OWNER_MARKER_FILE, OwnerMarker,
    OwnershipReceipt, PROFILE_NAME, SessionMode, SessionReceipt, create_managed_profile,
    park_managed_profile, prepare_profile_session, read_optional_json, restore_session,
};
use nan_harness_core::{SecretRef, SecretStore, SecretValue};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::TcpListener;

/// The provider is never contacted; the discard port keeps that explicit.
const PROVIDER_BASE_URL: &str = "http://127.0.0.1:9/v1";
const PROVIDER_CREDENTIAL: &str = "provider-secret";
const CREDENTIAL_REFERENCE: &str = "provider_key";
const SELECTED_MODEL: &str = "test-selected-model";
const OTHER_MODEL: &str = "test-other-model";
const DIAGNOSTIC_PREFIX: &str = "nan-diagnostic-";
const ENV_BEGIN: &str = "# nan-harness:begin hermes-desktop-session";
const ENV_END: &str = "# nan-harness:end hermes-desktop-session";
const USER_ENV: &str = "NAN_API_KEY=user-owned-value\n";
const SENTINEL: &str = "user sentinel\n";
const SENTINEL_FILE: &str = "user-notes.txt";

/// Generous enough that a slow machine never fails the case, short enough that
/// a hung gateway or filesystem wait is reported as a failure.
const CASE_DEADLINE: Duration = Duration::from_secs(30);
const PORT_RETRY_ATTEMPTS: u32 = 40;
const PORT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

fn paths() -> (tempfile::TempDir, DesktopPaths) {
    let root = tempfile::tempdir().expect("temporary root");
    let paths = DesktopPaths::for_test(root.path());
    (root, paths)
}

/// A resolved configuration built directly from synthetic parts, so no
/// environment, credential store, or resolver participates in the test.
fn synthetic_config() -> nan_harness_runtime::ResolvedConfig {
    let reference = SecretRef::new(CREDENTIAL_REFERENCE).expect("credential reference");
    let mut secrets = SecretStore::new();
    secrets.insert(
        reference.clone(),
        SecretValue::new(PROVIDER_CREDENTIAL).expect("provider credential"),
    );
    nan_harness_runtime::ResolvedConfig {
        provider_base_url: PROVIDER_BASE_URL.to_owned(),
        provider_credential_ref: reference,
        secrets,
    }
}

/// The selected model is deliberately not the first entry, so the written
/// configuration cannot pass by echoing the catalog order.
fn models() -> Vec<CodingModelProfile> {
    vec![
        CodingModelProfile::generic(OTHER_MODEL),
        CodingModelProfile::generic(SELECTED_MODEL),
    ]
}

async fn bounded<T>(operation: impl Future<Output = T>) -> T {
    tokio::time::timeout(CASE_DEADLINE, operation)
        .await
        .expect("profile preparation should complete within the test deadline")
}

fn parse_yaml(path: &Path) -> serde_yaml_ng::Value {
    let contents = fs::read_to_string(path).expect("profile configuration");
    serde_yaml_ng::from_str(&contents).expect("profile configuration should be valid YAML")
}

fn session_receipt(paths: &DesktopPaths) -> SessionReceipt {
    read_optional_json::<SessionReceipt>(&paths.session_receipt)
        .expect("session receipt read")
        .expect("preparation should record a session receipt")
}

fn recorded_gateway_port(paths: &DesktopPaths) -> u16 {
    read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)
        .expect("ownership receipt read")
        .expect("persistent preparation should own a receipt")
        .gateway_port
        .expect("persistent preparation should record a stable port")
}

/// Confirms the port the gateway held is bindable again. Closing a listener is
/// observable only once the operating system publishes it, so the check retries
/// within a bounded window instead of asserting on the first attempt.
async fn assert_port_is_reusable(port: u16) {
    for _ in 0..PORT_RETRY_ATTEMPTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            drop(listener);
            return;
        }
        tokio::time::sleep(PORT_RETRY_INTERVAL).await;
    }
    panic!("the gateway should release port {port} when it stops");
}

/// Locates the diagnostic profile the preparation created, and proves it
/// created exactly one.
fn single_diagnostic_profile(paths: &DesktopPaths) -> PathBuf {
    let mut profiles = diagnostic_profiles(paths);
    assert_eq!(
        profiles.len(),
        1,
        "diagnostic preparation should own exactly one profile"
    );
    profiles.pop().expect("diagnostic profile")
}

fn diagnostic_profiles(paths: &DesktopPaths) -> Vec<PathBuf> {
    fs::read_dir(&paths.profiles_root)
        .expect("profiles directory")
        .map(|entry| entry.expect("profile entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(DIAGNOSTIC_PREFIX))
        })
        .collect()
}

/// Creates a real owned profile, parked as `ensure_managed_profile` leaves it,
/// so a failing case starts from genuine ownership state.
fn owned_parked_profile(paths: &DesktopPaths) -> PathBuf {
    create_managed_profile(paths).expect("owned managed profile");
    paths.parked_profile.clone()
}

fn assert_profile_is_parked(paths: &DesktopPaths) {
    assert!(
        paths.parked_profile.is_dir(),
        "a failed preparation should leave the profile parked"
    );
    assert!(
        paths.managed_profile.is_file(),
        "parking should restore the visibility guard"
    );
}

#[tokio::test]
async fn diagnostic_preparation_owns_a_throwaway_profile_without_a_gateway() {
    let (_root, paths) = paths();
    let config = synthetic_config();

    let gateway = bounded(prepare_profile_session(
        true,
        &paths,
        &config,
        &models(),
        SELECTED_MODEL,
        true,
    ))
    .await
    .expect("diagnostic preparation should succeed");

    assert!(
        gateway.is_none(),
        "diagnostic preparation must not start a gateway"
    );
    let profile = single_diagnostic_profile(&paths);
    let marker = read_optional_json::<OwnerMarker>(&profile.join(OWNER_MARKER_FILE))
        .expect("owner marker read")
        .expect("the diagnostic profile should be marked as owned");
    assert_eq!(marker.owner_id, "diagnostic");

    let document = parse_yaml(&profile.join("config.yaml"));
    assert_eq!(document["model"]["default"], SELECTED_MODEL);
    assert_eq!(document["model"]["provider"], "nan");
    assert_eq!(document["providers"]["nan"]["base_url"], PROVIDER_BASE_URL);
    assert_eq!(document["providers"]["nan"]["model"], SELECTED_MODEL);
    assert!(document["providers"]["nan"]["models"][OTHER_MODEL].is_mapping());
    assert!(
        !profile.join("plugins/web/nan_harness/provider.py").exists(),
        "search runs through the gateway, so diagnostic mode leaves it uninstalled"
    );
    assert_eq!(
        fs::read_to_string(profile.join(".env")).expect("diagnostic environment"),
        format!("{ENV_BEGIN}\nNAN_API_KEY=\"{PROVIDER_CREDENTIAL}\"\n{ENV_END}\n")
    );
    assert_eq!(
        fs::read_to_string(&paths.active_profile).expect("active selection"),
        format!(
            "{{\n  \"profile\": \"{}\"\n}}",
            profile.file_name().expect("profile name").to_string_lossy()
        )
    );

    let receipt = session_receipt(&paths);
    assert_eq!(receipt.mode, SessionMode::Diagnostic);
    assert_eq!(receipt.profile, profile);
    assert!(
        !paths.ownership_receipt.exists() && !paths.managed_profile.exists(),
        "diagnostic preparation must not touch the persistent profile"
    );

    restore_session(&paths).expect("explicit restore");

    assert!(diagnostic_profiles(&paths).is_empty());
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
    assert!(!paths.active_profile.exists());
}

#[tokio::test]
async fn persistent_preparation_supports_gateway_shutdown_and_profile_restore() {
    let (_root, paths) = paths();
    let config = synthetic_config();

    let mut gateway = bounded(prepare_profile_session(
        false,
        &paths,
        &config,
        &models(),
        SELECTED_MODEL,
        true,
    ))
    .await
    .expect("persistent preparation should succeed");
    let (base_url, session_token) = {
        let running = gateway
            .as_ref()
            .expect("persistent preparation should return a running gateway");
        (
            running.client_base_url(),
            running.with_session_token(str::to_owned),
        )
    };
    bounded(gateway.take().expect("owned gateway").shutdown())
        .await
        .expect("the gateway should stop cleanly");

    let port = recorded_gateway_port(&paths);
    assert_eq!(base_url, format!("http://127.0.0.1:{port}/v1"));
    let document = parse_yaml(&paths.managed_profile.join("config.yaml"));
    assert_eq!(document["model"]["default"], SELECTED_MODEL);
    assert_eq!(document["providers"]["nan"]["base_url"], base_url);
    assert_eq!(document["providers"]["nan"]["key_env"], "NAN_API_KEY");
    assert_eq!(document["web"]["search_backend"], "nan-harness");
    let provider = fs::read_to_string(
        paths
            .managed_profile
            .join("plugins/web/nan_harness/provider.py"),
    )
    .expect("search provider");
    assert!(provider.contains(&format!("{base_url}/search")));

    let receipt = session_receipt(&paths);
    assert_eq!(receipt.mode, SessionMode::Persistent);
    assert_eq!(receipt.profile, paths.managed_profile);
    assert_eq!(
        fs::read_to_string(paths.managed_profile.join(".env")).expect("session environment"),
        format!("{ENV_BEGIN}\nNAN_API_KEY=\"{session_token}\"\n{ENV_END}\n")
    );
    assert_ne!(
        session_token, PROVIDER_CREDENTIAL,
        "the profile receives the gateway token, never the provider credential"
    );
    assert_eq!(
        fs::read_to_string(&paths.active_profile).expect("active selection"),
        format!("{{\n  \"profile\": \"{PROFILE_NAME}\"\n}}")
    );

    assert_port_is_reusable(port).await;
    restore_session(&paths).expect("explicit restore");
    park_managed_profile(&paths).expect("explicit park");

    assert!(!paths.parked_profile.join(".env").exists());
    assert!(!paths.session_receipt.exists());
    assert!(!paths.backup_directory.exists());
    assert!(!paths.active_profile.exists());
    assert_profile_is_parked(&paths);
    assert_eq!(
        recorded_gateway_port(&paths),
        port,
        "the stable port outlives the session so the next launch reuses it"
    );
}

#[tokio::test]
async fn an_unwritable_profile_config_releases_the_gateway_and_parks_the_profile() {
    let (_root, paths) = paths();
    let config = synthetic_config();
    let parked = owned_parked_profile(&paths);
    fs::create_dir(parked.join("config.yaml")).expect("config path occupied by a directory");
    fs::write(parked.join(SENTINEL_FILE), SENTINEL).expect("unrelated user file");

    let error = bounded(prepare_profile_session(
        false,
        &paths,
        &config,
        &models(),
        SELECTED_MODEL,
        true,
    ))
    .await
    .expect_err("an unwritable profile configuration should fail preparation");

    assert!(matches!(error, HermesDesktopError::ReadProfileConfig(_)));
    assert_port_is_reusable(recorded_gateway_port(&paths)).await;
    assert!(
        !paths.session_receipt.exists(),
        "a failed preparation should not begin a session"
    );
    assert_profile_is_parked(&paths);
    assert_eq!(
        fs::read_to_string(parked.join(SENTINEL_FILE)).expect("preserved user file"),
        SENTINEL
    );
    assert!(!paths.parked_profile.join(".env").exists());
}

#[tokio::test]
async fn a_conflicting_profile_credential_releases_the_gateway_and_parks_the_profile() {
    let (_root, paths) = paths();
    let config = synthetic_config();
    let parked = owned_parked_profile(&paths);
    fs::write(parked.join(".env"), USER_ENV).expect("user credential");

    let error = bounded(prepare_profile_session(
        false,
        &paths,
        &config,
        &models(),
        SELECTED_MODEL,
        true,
    ))
    .await
    .expect_err("a user credential in the profile should fail preparation");

    assert!(matches!(
        error,
        HermesDesktopError::ProfileCredentialConflict
    ));
    assert_port_is_reusable(recorded_gateway_port(&paths)).await;
    assert!(
        !paths.session_receipt.exists(),
        "a failed preparation should not begin a session"
    );
    assert_profile_is_parked(&paths);
    assert_eq!(
        fs::read_to_string(paths.parked_profile.join(".env")).expect("preserved user credential"),
        USER_ENV
    );
    assert!(!paths.active_profile.exists());
}
