use futures_util::StreamExt as _;
use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;
use url::Url;

pub const COMPATIBILITY_MANIFEST_ENVIRONMENT_VARIABLE: &str = "NAN_COMPATIBILITY_MANIFEST_URL";
pub const DISABLE_COMPATIBILITY_REFRESH_ENVIRONMENT_VARIABLE: &str = "NAN_NO_COMPATIBILITY_CHECK";
const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";
const BUILD_COMPATIBILITY_MANIFEST_URL: Option<&str> =
    option_env!("NAN_COMPATIBILITY_MANIFEST_URL");
const STATE_SCHEMA_VERSION: u8 = 2;
const MANIFEST_SCHEMA_VERSION: u8 = 2;
const CHECK_INTERVAL: Duration = Duration::from_hours(24);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_REDIRECTS: usize = 3;
const MAX_MANIFEST_SIZE: usize = 1024 * 1024;
const STATE_FILE_NAME: &str = "compatibility.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationManifest {
    pub schema_version: u8,
    pub releases: Vec<VerificationRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationRelease {
    pub nan_harness_version: Version,
    #[serde(alias = "harnesses")]
    pub verifications: Vec<VerificationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationEntry {
    pub id: String,
    #[serde(default)]
    pub last_compatible_version: Option<Version>,
    #[serde(default)]
    pub compatible_at: Option<String>,
    #[serde(default)]
    pub last_live_verified_version: Option<Version>,
    #[serde(default)]
    pub live_verified_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    Disabled,
    Cached,
    Updated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompatibilityState {
    schema_version: u8,
    last_checked_unix_seconds: Option<u64>,
    cached_manifest: Option<VerificationManifest>,
}

impl Default for CompatibilityState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            last_checked_unix_seconds: None,
            cached_manifest: None,
        }
    }
}

#[derive(Debug, Clone)]
struct CompatibilityStateStore {
    directory: PathBuf,
    path: PathBuf,
}

impl CompatibilityStateStore {
    fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let path = directory.join(STATE_FILE_NAME);
        Self { directory, path }
    }

    fn from_environment() -> Result<Self, CompatibilityError> {
        if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
            return Ok(Self::new(directory));
        }
        platform_config_directory()
            .map(Self::new)
            .ok_or(CompatibilityError::MissingConfigDirectory)
    }

    fn load(&self) -> Result<CompatibilityState, CompatibilityError> {
        match fs::read(&self.path) {
            Ok(contents) => {
                let state: CompatibilityState =
                    serde_json::from_slice(&contents).map_err(CompatibilityError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(CompatibilityError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(CompatibilityState::default())
            }
            Err(error) => Err(CompatibilityError::ReadState(error)),
        }
    }

    fn save(&self, state: &CompatibilityState) -> Result<(), CompatibilityError> {
        fs::create_dir_all(&self.directory).map_err(CompatibilityError::CreateConfigDirectory)?;
        let payload =
            serde_json::to_vec_pretty(state).map_err(CompatibilityError::SerializeState)?;
        atomic_write(&self.path, &payload).map_err(CompatibilityError::WriteState)
    }
}

/// Refreshes the compatibility evidence overlay without replacing the running binary.
///
/// # Errors
///
/// Returns [`CompatibilityError`] when the configured feed or its local cache cannot be used.
pub async fn refresh_compatibility_manifest() -> Result<RefreshOutcome, CompatibilityError> {
    if !automatic_refresh_enabled() {
        return Ok(RefreshOutcome::Disabled);
    }
    let Some(url) = compatibility_manifest_url() else {
        return Ok(RefreshOutcome::Disabled);
    };
    let store = CompatibilityStateStore::from_environment()?;
    let base = crate::discovery::bundled_compatibility_manifest()
        .map_err(|error| CompatibilityError::InvalidEmbeddedManifest(error.to_string()))?;
    refresh_store(&url, &store, &base).await
}

async fn refresh_store(
    url: &str,
    store: &CompatibilityStateStore,
    base: &CompatibilityManifest,
) -> Result<RefreshOutcome, CompatibilityError> {
    let mut state = store.load().unwrap_or_default();
    if cache_is_fresh(&state) {
        return Ok(RefreshOutcome::Cached);
    }
    let manifest = fetch_manifest(url, base).await?;
    state.last_checked_unix_seconds = Some(unix_seconds()?);
    state.cached_manifest = Some(manifest);
    store.save(&state)?;
    Ok(RefreshOutcome::Updated)
}

#[must_use]
pub fn automatic_refresh_enabled() -> bool {
    !environment_flag(DISABLE_COMPATIBILITY_REFRESH_ENVIRONMENT_VARIABLE)
        && env::var_os("CI").is_none()
}

#[must_use]
pub fn compatibility_manifest_url() -> Option<String> {
    env::var(COMPATIBILITY_MANIFEST_ENVIRONMENT_VARIABLE)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| BUILD_COMPATIBILITY_MANIFEST_URL.map(ToOwned::to_owned))
}

pub(crate) fn apply_cached_verifications(manifest: &mut CompatibilityManifest) {
    let Ok(store) = CompatibilityStateStore::from_environment() else {
        return;
    };
    let Ok(state) = store.load() else {
        return;
    };
    if let Some(cached) = state.cached_manifest {
        let version = Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is valid");
        if let Some(release) = select_release(&cached, &version) {
            apply_verifications(manifest, release);
        }
    }
}

fn select_release<'a>(
    manifest: &'a VerificationManifest,
    version: &Version,
) -> Option<&'a VerificationRelease> {
    manifest
        .releases
        .iter()
        .find(|release| &release.nan_harness_version == version)
}

fn apply_verifications(manifest: &mut CompatibilityManifest, release: &VerificationRelease) {
    for verification in &release.verifications {
        let Ok(id) = verification.id.parse::<HarnessKind>() else {
            continue;
        };
        let Some(entry) = manifest.harnesses.iter_mut().find(|entry| entry.id == id) else {
            continue;
        };
        if let Some(version) = &verification.last_compatible_version {
            entry.last_compatible_version =
                entry.last_compatible_version.clone().max(version.clone());
        }
        if let Some(timestamp) = &verification.compatible_at
            && timestamp > &entry.compatible_at
        {
            entry.compatible_at.clone_from(timestamp);
        }
        if let Some(version) = &verification.last_live_verified_version {
            entry.last_live_verified_version = match entry.last_live_verified_version.take() {
                Some(current) => Some(current.max(version.clone())),
                None => Some(version.clone()),
            };
        }
        if let Some(timestamp) = &verification.live_verified_at
            && entry
                .live_verified_at
                .as_ref()
                .is_none_or(|current| timestamp > current)
        {
            entry.live_verified_at = Some(timestamp.clone());
        }
    }
}

async fn fetch_manifest(
    url: &str,
    base: &CompatibilityManifest,
) -> Result<VerificationManifest, CompatibilityError> {
    validate_url(url)?;
    let client = reqwest::Client::builder()
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(compatibility_redirect_policy())
        .build()
        .map_err(CompatibilityError::BuildClient)?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(fetch_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(CompatibilityError::ManifestStatus(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(MAX_MANIFEST_SIZE).unwrap_or(u64::MAX))
    {
        return Err(CompatibilityError::ManifestTooLarge);
    }
    let mut contents = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(fetch_error)?;
        if contents.len().saturating_add(chunk.len()) > MAX_MANIFEST_SIZE {
            return Err(CompatibilityError::ManifestTooLarge);
        }
        contents.extend_from_slice(&chunk);
    }
    let manifest: VerificationManifest =
        serde_json::from_slice(&contents).map_err(CompatibilityError::ParseManifest)?;
    validate_manifest(&manifest, base)?;
    Ok(manifest)
}

fn fetch_error(error: reqwest::Error) -> CompatibilityError {
    CompatibilityError::FetchManifest(error.without_url())
}

fn validate_manifest(
    manifest: &VerificationManifest,
    base: &CompatibilityManifest,
) -> Result<(), CompatibilityError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(CompatibilityError::UnsupportedManifestSchema(
            manifest.schema_version,
        ));
    }
    let mut release_versions = BTreeSet::new();
    for release in &manifest.releases {
        if !release_versions.insert(release.nan_harness_version.clone()) {
            return Err(CompatibilityError::DuplicateRelease(
                release.nan_harness_version.clone(),
            ));
        }
        let mut ids = BTreeSet::new();
        for verification in &release.verifications {
            let Ok(id) = verification.id.parse::<HarnessKind>() else {
                continue;
            };
            if !ids.insert(id) {
                return Err(CompatibilityError::DuplicateHarness(id));
            }
            let Some(entry) = base.entry(id) else {
                continue;
            };
            if let Some(version) = &verification.last_compatible_version
                && version < &entry.minimum_version
            {
                return Err(CompatibilityError::VersionBelowMinimum {
                    harness: id,
                    version: version.clone(),
                    minimum: entry.minimum_version.clone(),
                });
            }
            if let Some(version) = &verification.last_live_verified_version
                && version < &entry.minimum_version
            {
                return Err(CompatibilityError::LiveVersionBelowMinimum {
                    harness: id,
                    version: version.clone(),
                    minimum: entry.minimum_version.clone(),
                });
            }
        }
    }
    Ok(())
}

fn cache_is_fresh(state: &CompatibilityState) -> bool {
    let Some(last_checked) = state.last_checked_unix_seconds else {
        return false;
    };
    let Ok(now) = unix_seconds() else {
        return false;
    };
    now.checked_sub(last_checked)
        .is_some_and(|age| age < CHECK_INTERVAL.as_secs())
        && state.cached_manifest.is_some()
}

fn validate_url(value: &str) -> Result<(), CompatibilityError> {
    let url = Url::parse(value).map_err(|source| CompatibilityError::InvalidUrl { source })?;
    let local_http = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"));
    if url.scheme() != "https" && !local_http {
        return Err(CompatibilityError::InsecureUrl);
    }
    Ok(())
}

fn compatibility_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("compatibility redirect limit exceeded");
        }
        let Some(initial_url) = attempt.previous().first() else {
            return attempt.stop();
        };
        if redirect_is_allowed(initial_url, attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn redirect_is_allowed(initial_url: &Url, next_url: &Url) -> bool {
    if initial_url.scheme() != "https"
        || next_url.scheme() != "https"
        || !next_url.username().is_empty()
        || next_url.password().is_some()
    {
        return false;
    }
    let same_origin = initial_url.host_str() == next_url.host_str()
        && initial_url.port_or_known_default() == next_url.port_or_known_default();
    let github_release_asset = initial_url.host_str() == Some("github.com")
        && next_url.host_str() == Some("release-assets.githubusercontent.com");
    same_origin || github_release_asset
}

fn environment_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn unix_seconds() -> Result<u64, CompatibilityError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(CompatibilityError::SystemClock)
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), std::io::Error> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let mut temporary = TempFileBuilder::new().prefix(".nan-").tempfile_in(parent)?;
    temporary.write_all(payload)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn platform_config_directory() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/nan-harness"))
    }
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config/nan-harness"))
            })
    }
}

#[derive(Debug, Error)]
pub enum CompatibilityError {
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not build the compatibility metadata client: {0}")]
    BuildClient(reqwest::Error),
    #[error("the compatibility manifest URL is invalid: {source}")]
    InvalidUrl { source: url::ParseError },
    #[error("the compatibility manifest URL must use HTTPS")]
    InsecureUrl,
    #[error("could not fetch compatibility metadata: {0}")]
    FetchManifest(reqwest::Error),
    #[error("the compatibility server returned HTTP {0}")]
    ManifestStatus(u16),
    #[error("the compatibility manifest exceeds the 1 MiB safety limit")]
    ManifestTooLarge,
    #[error("the compatibility manifest is not valid JSON: {0}")]
    ParseManifest(serde_json::Error),
    #[error("compatibility manifest schema {0} is not supported")]
    UnsupportedManifestSchema(u8),
    #[error("compatibility manifest contains duplicate release {0}")]
    DuplicateRelease(Version),
    #[error("compatibility manifest contains duplicate entry for {0}")]
    DuplicateHarness(HarnessKind),
    #[error(
        "compatibility manifest reports {harness} version {version}, below embedded minimum {minimum}"
    )]
    VersionBelowMinimum {
        harness: HarnessKind,
        version: Version,
        minimum: Version,
    },
    #[error(
        "compatibility manifest reports {harness} live version {version}, below embedded minimum {minimum}"
    )]
    LiveVersionBelowMinimum {
        harness: HarnessKind,
        version: Version,
        minimum: Version,
    },
    #[error("embedded compatibility manifest is invalid: {0}")]
    InvalidEmbeddedManifest(String),
    #[error("could not read compatibility settings: {0}")]
    ReadState(std::io::Error),
    #[error("compatibility settings are not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("compatibility settings schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not create the nan-harness configuration directory: {0}")]
    CreateConfigDirectory(std::io::Error),
    #[error("could not serialize compatibility settings: {0}")]
    SerializeState(serde_json::Error),
    #[error("could not write compatibility settings: {0}")]
    WriteState(std::io::Error),
    #[error("the system clock is before the Unix epoch: {0}")]
    SystemClock(std::time::SystemTimeError),
}

impl CompatibilityError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingConfigDirectory
            | Self::ReadState(_)
            | Self::ParseState(_)
            | Self::UnsupportedStateSchema(_)
            | Self::CreateConfigDirectory(_)
            | Self::SerializeState(_)
            | Self::WriteState(_)
            | Self::SystemClock(_) => "NH-COMPATIBILITY-001",
            Self::BuildClient(_) | Self::FetchManifest(_) | Self::ManifestStatus(_) => {
                "NH-COMPATIBILITY-002"
            }
            Self::InvalidUrl { .. }
            | Self::InsecureUrl
            | Self::ManifestTooLarge
            | Self::ParseManifest(_)
            | Self::UnsupportedManifestSchema(_)
            | Self::DuplicateRelease(_)
            | Self::DuplicateHarness(_)
            | Self::VersionBelowMinimum { .. }
            | Self::LiveVersionBelowMinimum { .. }
            | Self::InvalidEmbeddedManifest(_) => "NH-COMPATIBILITY-003",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompatibilityError, CompatibilityState, CompatibilityStateStore, MAX_MANIFEST_SIZE,
        RefreshOutcome, VerificationEntry, VerificationManifest, VerificationRelease,
        apply_verifications, cache_is_fresh, fetch_manifest, redirect_is_allowed, refresh_store,
        select_release,
    };
    use axum::Json;
    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::response::Redirect;
    use axum::routing::get;
    use nan_harness_core::{CompatibilityManifest, HarnessKind};
    use semver::Version;
    use serde_json::json;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::net::TcpListener;
    use url::Url;

    #[test]
    fn redirects_allow_only_expected_https_origins() {
        let github = Url::parse(
            "https://github.com/DavidLMS/nan-harness/releases/download/compatibility/compatibility.json",
        )
        .expect("GitHub URL");
        let release_asset = Url::parse(
            "https://release-assets.githubusercontent.com/github-production-release-asset/file",
        )
        .expect("release asset URL");
        let same_origin = Url::parse(
            "https://github.com/DavidLMS/nan-harness/releases/download/compatibility/feed.json",
        )
        .expect("same-origin URL");

        assert!(redirect_is_allowed(&github, &release_asset));
        assert!(redirect_is_allowed(&github, &same_origin));
        for rejected in [
            "http://release-assets.githubusercontent.com/file",
            "https://user@release-assets.githubusercontent.com/file",
            "https://release-assets.githubusercontent.com.evil.example/file",
            "https://objects.githubusercontent.com/file",
        ] {
            let rejected = Url::parse(rejected).expect("rejected URL should parse");
            assert!(!redirect_is_allowed(&github, &rejected), "{rejected}");
        }
    }

    #[test]
    fn overlay_only_advances_known_compatible_versions() {
        let mut base = base_manifest();
        let original_policy = base.policy.clone();
        let original_minimum = base
            .entry(HarnessKind::Codex)
            .expect("Codex entry")
            .minimum_version
            .clone();
        let remote = VerificationManifest {
            schema_version: 2,
            releases: vec![VerificationRelease {
                nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                verifications: vec![VerificationEntry {
                    id: "codex".to_owned(),
                    last_compatible_version: Some(Version::new(0, 147, 0)),
                    compatible_at: Some("2026-08-19T08:00:00Z".to_owned()),
                    last_live_verified_version: None,
                    live_verified_at: None,
                }],
            }],
        };

        apply_verifications(&mut base, &remote.releases[0]);

        let codex = base.entry(HarnessKind::Codex).expect("Codex entry");
        assert_eq!(codex.last_compatible_version, Version::new(0, 147, 0));
        assert_eq!(codex.minimum_version, original_minimum);
        assert_eq!(base.policy, original_policy);
    }

    #[test]
    fn overlay_never_regresses_the_embedded_compatible_version() {
        let mut base = base_manifest();
        let remote = VerificationRelease {
            nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
            verifications: vec![VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 145, 0)),
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            }],
        };

        apply_verifications(&mut base, &remote);

        assert_eq!(
            base.entry(HarnessKind::Codex)
                .expect("Codex entry")
                .last_compatible_version,
            Version::new(0, 146, 0)
        );
    }

    #[test]
    fn release_selection_is_exact_and_unknown_harnesses_are_ignored() {
        let mut base = base_manifest();
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        let feed = VerificationManifest {
            schema_version: 2,
            releases: vec![
                VerificationRelease {
                    nan_harness_version: current.clone(),
                    verifications: vec![
                        VerificationEntry {
                            id: "codex".to_owned(),
                            last_compatible_version: Some(Version::new(0, 147, 0)),
                            compatible_at: None,
                            last_live_verified_version: None,
                            live_verified_at: None,
                        },
                        VerificationEntry {
                            id: "unknown-harness".to_owned(),
                            last_compatible_version: Some(Version::new(99, 0, 0)),
                            compatible_at: None,
                            last_live_verified_version: None,
                            live_verified_at: None,
                        },
                    ],
                },
                VerificationRelease {
                    nan_harness_version: Version::new(99, 0, 0),
                    verifications: vec![VerificationEntry {
                        id: "unknown-harness".to_owned(),
                        last_compatible_version: Some(Version::new(99, 0, 0)),
                        compatible_at: None,
                        last_live_verified_version: None,
                        live_verified_at: None,
                    }],
                },
            ],
        };

        let release = select_release(&feed, &current).expect("current release should match");
        apply_verifications(&mut base, release);
        assert_eq!(
            base.entry(HarnessKind::Codex)
                .expect("Codex entry")
                .last_compatible_version,
            Version::new(0, 147, 0)
        );
        assert!(select_release(&feed, &Version::new(1, 0, 0)).is_none());
    }

    #[test]
    fn cache_state_round_trips() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = CompatibilityStateStore::new(directory.path());
        let state = CompatibilityState {
            schema_version: 2,
            last_checked_unix_seconds: Some(42),
            cached_manifest: Some(VerificationManifest {
                schema_version: 2,
                releases: vec![VerificationRelease {
                    nan_harness_version: Version::parse(env!("CARGO_PKG_VERSION")).unwrap(),
                    verifications: vec![VerificationEntry {
                        id: "claude-code".to_owned(),
                        last_compatible_version: Some(Version::new(2, 1, 234)),
                        compatible_at: Some("2026-08-19".to_owned()),
                        last_live_verified_version: None,
                        live_verified_at: None,
                    }],
                }],
            }),
        };
        store.save(&state).expect("state should save");
        assert_eq!(store.load().expect("state should load"), state);
    }

    #[test]
    fn future_cache_timestamps_are_not_fresh() {
        let state = CompatibilityState {
            schema_version: 2,
            last_checked_unix_seconds: Some(u64::MAX),
            cached_manifest: Some(VerificationManifest {
                schema_version: 2,
                releases: Vec::new(),
            }),
        };

        assert!(!cache_is_fresh(&state));
    }

    #[tokio::test]
    async fn remote_manifest_is_validated_and_downloaded() {
        let payload = json!({
            "schemaVersion": 2,
            "releases": [{
                "nanHarnessVersion": env!("CARGO_PKG_VERSION"),
                "verifications": [{
                    "id": "codex",
                    "lastCompatibleVersion": "0.147.0",
                    "compatibleAt": "2026-08-19T08:00:00Z"
                }]
            }]
        });
        let app = Router::new().route(
            "/compatibility.json",
            get({
                let payload = Arc::new(payload);
                move || {
                    let payload = Arc::clone(&payload);
                    async move { Json((*payload).clone()) }
                }
            }),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        tokio::spawn(axum::serve(listener, app).into_future());

        let url = format!("http://{address}/compatibility.json");
        let base = base_manifest();
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = CompatibilityStateStore::new(directory.path());
        let outcome = refresh_store(&url, &store, &base)
            .await
            .expect("remote manifest should validate and cache");
        assert_eq!(outcome, RefreshOutcome::Updated);
        let state = store.load().expect("cached state should load");
        assert_eq!(
            state.cached_manifest.expect("cached manifest").releases[0].verifications[0]
                .last_compatible_version,
            Some(Version::new(0, 147, 0))
        );
        assert_eq!(
            refresh_store(&url, &store, &base)
                .await
                .expect("fresh cache should be reused"),
            RefreshOutcome::Cached
        );
    }

    #[tokio::test]
    async fn remote_manifest_redirects_are_not_followed() {
        let target_reached = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/compatibility.json") }),
            )
            .route(
                "/compatibility.json",
                get({
                    let target_reached = Arc::clone(&target_reached);
                    move || {
                        let target_reached = Arc::clone(&target_reached);
                        async move {
                            target_reached.store(true, Ordering::SeqCst);
                            Json(json!({
                                "schemaVersion": 2,
                                "releases": []
                            }))
                        }
                    }
                }),
            );
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        tokio::spawn(axum::serve(listener, app).into_future());
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = CompatibilityStateStore::new(directory.path());

        let error = refresh_store(
            &format!("http://{address}/redirect"),
            &store,
            &base_manifest(),
        )
        .await
        .expect_err("redirect should be rejected");

        assert!(matches!(error, CompatibilityError::ManifestStatus(307)));
        assert!(!target_reached.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn remote_manifest_stream_is_bounded_while_downloading() {
        let app = Router::new().route(
            "/compatibility.json",
            get(|| async {
                let chunks = vec![
                    Ok::<_, Infallible>(Bytes::from(vec![b' '; MAX_MANIFEST_SIZE / 2])),
                    Ok(Bytes::from(vec![b' '; MAX_MANIFEST_SIZE / 2])),
                    Ok(Bytes::from_static(b" ")),
                ];
                axum::response::Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
            }),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        tokio::spawn(axum::serve(listener, app).into_future());
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = CompatibilityStateStore::new(directory.path());

        let error = refresh_store(
            &format!("http://{address}/compatibility.json"),
            &store,
            &base_manifest(),
        )
        .await
        .expect_err("oversized stream should be rejected");

        assert!(matches!(error, CompatibilityError::ManifestTooLarge));
    }

    #[tokio::test]
    async fn remote_manifest_errors_do_not_retain_the_request_url() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        drop(listener);

        let error = fetch_manifest(
            &format!("http://{address}/compatibility.json?token=nan-secret"),
            &base_manifest(),
        )
        .await
        .expect_err("closed endpoint should fail");

        let CompatibilityError::FetchManifest(source) = &error else {
            panic!("expected a fetch error, received {error}");
        };
        assert!(source.url().is_none());
        assert!(!error.to_string().contains("nan-secret"));
    }

    fn base_manifest() -> CompatibilityManifest {
        crate::discovery::bundled_compatibility_manifest().expect("embedded manifest")
    }
}
