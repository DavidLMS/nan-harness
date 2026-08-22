use futures_util::StreamExt as _;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::{Builder as TempFileBuilder, TempPath};
use thiserror::Error;
use url::Url;

pub const UPDATE_MANIFEST_ENVIRONMENT_VARIABLE: &str = "NAN_UPDATE_MANIFEST_URL";
pub const DISABLE_UPDATE_CHECK_ENVIRONMENT_VARIABLE: &str = "NAN_NO_UPDATE_CHECK";
pub const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";

const UPDATE_STATE_SCHEMA_VERSION: u8 = 1;
const RELEASE_MANIFEST_SCHEMA_VERSION: u8 = 1;
const CHECK_INTERVAL: Duration = Duration::from_hours(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_MANIFEST_SIZE: usize = 1024 * 1024;
const MAX_BINARY_SIZE: u64 = 128 * 1024 * 1024;
const BUILD_UPDATE_MANIFEST_URL: Option<&str> = option_env!("NAN_UPDATE_MANIFEST_URL");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseArtifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u8,
    pub version: Version,
    pub notes_url: String,
    pub artifacts: Vec<ReleaseArtifact>,
}

impl ReleaseManifest {
    fn validate(&self) -> Result<(), UpdateError> {
        if self.schema_version != RELEASE_MANIFEST_SCHEMA_VERSION {
            return Err(UpdateError::UnsupportedManifestSchema(self.schema_version));
        }
        validate_https_url(&self.notes_url, "release notes")?;
        if self.artifacts.is_empty() {
            return Err(UpdateError::EmptyArtifactCatalog);
        }
        for artifact in &self.artifacts {
            validate_https_url(&artifact.url, "release artifact")?;
            validate_sha256(&artifact.sha256)?;
        }
        Ok(())
    }

    fn artifact_for_current_target(&self) -> Result<&ReleaseArtifact, UpdateError> {
        let target = current_target();
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
            .ok_or_else(|| UpdateError::MissingArtifact(target.to_owned()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateState {
    schema_version: u8,
    last_checked_unix_seconds: Option<u64>,
    skipped_version: Option<Version>,
    cached_release: Option<ReleaseManifest>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            schema_version: UPDATE_STATE_SCHEMA_VERSION,
            last_checked_unix_seconds: None,
            skipped_version: None,
            cached_release: None,
        }
    }
}

#[derive(Debug, Clone)]
struct UpdateStateStore {
    directory: PathBuf,
    path: PathBuf,
}

impl UpdateStateStore {
    fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let path = directory.join("update.json");
        Self { directory, path }
    }

    fn from_environment() -> Result<Self, UpdateError> {
        if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
            return Ok(Self::new(directory));
        }
        platform_config_directory()
            .map(Self::new)
            .ok_or(UpdateError::MissingConfigDirectory)
    }

    fn load(&self) -> Result<UpdateState, UpdateError> {
        match fs::read(&self.path) {
            Ok(contents) => {
                let state: UpdateState =
                    serde_json::from_slice(&contents).map_err(UpdateError::ParseState)?;
                if state.schema_version != UPDATE_STATE_SCHEMA_VERSION {
                    return Err(UpdateError::UnsupportedStateSchema(state.schema_version));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(UpdateState::default())
            }
            Err(error) => Err(UpdateError::ReadState(error)),
        }
    }

    fn save(&self, state: &UpdateState) -> Result<(), UpdateError> {
        fs::create_dir_all(&self.directory).map_err(UpdateError::CreateConfigDirectory)?;
        let payload = serde_json::to_vec_pretty(state).map_err(UpdateError::SerializeState)?;
        atomic_write(&self.path, &payload).map_err(UpdateError::WriteState)
    }
}

#[derive(Debug)]
pub struct UpdateManager {
    current_version: Version,
    manifest_url: Option<String>,
    client: reqwest::Client,
    state: UpdateStateStore,
}

impl UpdateManager {
    /// Builds the updater using the release channel embedded at build time or supplied through
    /// `NAN_UPDATE_MANIFEST_URL` for development and testing.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] when the running version, HTTP client, or configuration directory
    /// cannot be initialized.
    pub fn from_environment() -> Result<Self, UpdateError> {
        let manifest_url = env::var(UPDATE_MANIFEST_ENVIRONMENT_VARIABLE)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| BUILD_UPDATE_MANIFEST_URL.map(ToOwned::to_owned));
        Self::new(
            env!("CARGO_PKG_VERSION"),
            manifest_url,
            UpdateStateStore::from_environment()?,
        )
    }

    fn new(
        current_version: &str,
        manifest_url: Option<String>,
        state: UpdateStateStore,
    ) -> Result<Self, UpdateError> {
        let current_version = Version::parse(current_version).map_err(UpdateError::Version)?;
        if let Some(url) = manifest_url.as_deref() {
            validate_https_url(url, "update manifest")?;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(format!("nan/{current_version}"))
            .build()
            .map_err(UpdateError::BuildClient)?;
        Ok(Self {
            current_version,
            manifest_url,
            client,
            state,
        })
    }

    #[must_use]
    pub fn channel_available(&self) -> bool {
        self.manifest_url.is_some()
    }

    #[must_use]
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    #[must_use]
    pub fn automatic_checks_enabled() -> bool {
        !environment_flag(DISABLE_UPDATE_CHECK_ENVIRONMENT_VARIABLE) && env::var_os("CI").is_none()
    }

    /// Returns the newest release when it is newer than the running binary and has not been
    /// skipped. Cached metadata is reused for one day, while a deferred release remains visible
    /// on every interactive launch.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] when release metadata cannot be loaded, downloaded, or validated.
    pub async fn available_release(
        &self,
        force_refresh: bool,
        honor_skipped_version: bool,
    ) -> Result<Option<ReleaseManifest>, UpdateError> {
        if self.manifest_url.is_none() {
            return Err(UpdateError::UpdateChannelUnavailable);
        }
        let mut state = self.state.load().unwrap_or_default();
        let release = if !force_refresh && cache_is_fresh(&state) {
            state.cached_release.clone()
        } else {
            let release = self.fetch_release().await?;
            state.last_checked_unix_seconds = Some(unix_seconds()?);
            state.cached_release = Some(release.clone());
            if state
                .skipped_version
                .as_ref()
                .is_some_and(|skipped| release.version > *skipped)
            {
                state.skipped_version = None;
            }
            self.state.save(&state)?;
            Some(release)
        };
        let Some(release) = release else {
            return Ok(None);
        };
        if release.version <= self.current_version {
            return Ok(None);
        }
        if honor_skipped_version
            && state
                .skipped_version
                .as_ref()
                .is_some_and(|skipped| *skipped == release.version)
        {
            return Ok(None);
        }
        Ok(Some(release))
    }

    /// Suppresses one exact release while allowing later versions to prompt normally.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] when the updater state cannot be persisted.
    pub fn skip(&self, version: Version) -> Result<(), UpdateError> {
        let mut state = self.state.load().unwrap_or_default();
        state.skipped_version = Some(version);
        self.state.save(&state)
    }

    /// Downloads, verifies, executes, and atomically installs a release binary.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError`] when the artifact is unavailable, invalid, or cannot replace the
    /// current executable.
    pub async fn install(&self, release: &ReleaseManifest) -> Result<(), UpdateError> {
        release.validate()?;
        let artifact = release.artifact_for_current_target()?;
        let candidate = self.download(artifact).await?;
        let candidate_path: &Path = candidate.as_ref();
        verify_candidate(candidate_path, &release.version)?;
        self_replace::self_replace(candidate_path).map_err(UpdateError::ReplaceExecutable)?;
        candidate.close().map_err(UpdateError::RemoveCandidate)
    }

    async fn fetch_release(&self) -> Result<ReleaseManifest, UpdateError> {
        let url = self
            .manifest_url
            .as_deref()
            .ok_or(UpdateError::UpdateChannelUnavailable)?;
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(UpdateError::FetchManifest)?;
        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::ManifestStatus(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > u64::try_from(MAX_MANIFEST_SIZE).unwrap_or(u64::MAX))
        {
            return Err(UpdateError::ManifestTooLarge);
        }
        let mut contents = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(UpdateError::FetchManifest)?;
            if contents.len().saturating_add(chunk.len()) > MAX_MANIFEST_SIZE {
                return Err(UpdateError::ManifestTooLarge);
            }
            contents.extend_from_slice(&chunk);
        }
        let release = serde_json::from_slice::<ReleaseManifest>(&contents)
            .map_err(UpdateError::ParseManifest)?;
        release.validate()?;
        Ok(release)
    }

    async fn download(&self, artifact: &ReleaseArtifact) -> Result<TempPath, UpdateError> {
        let response = self
            .client
            .get(&artifact.url)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .send()
            .await
            .map_err(UpdateError::DownloadArtifact)?;
        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::ArtifactStatus(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BINARY_SIZE)
        {
            return Err(UpdateError::ArtifactTooLarge);
        }

        let mut builder = TempFileBuilder::new();
        builder.prefix("nan-update-");
        #[cfg(windows)]
        builder.suffix(".exe");
        let mut file = builder.tempfile().map_err(UpdateError::CreateCandidate)?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(UpdateError::DownloadArtifact)?;
            size = size
                .checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX))
                .ok_or(UpdateError::ArtifactTooLarge)?;
            if size > MAX_BINARY_SIZE {
                return Err(UpdateError::ArtifactTooLarge);
            }
            digest.update(&chunk);
            file.write_all(&chunk)
                .map_err(UpdateError::WriteCandidate)?;
        }
        file.flush().map_err(UpdateError::WriteCandidate)?;
        file.as_file()
            .sync_all()
            .map_err(UpdateError::WriteCandidate)?;
        let actual = hex_digest(digest.finalize());
        if !constant_time_hex_eq(&actual, &artifact.sha256) {
            return Err(UpdateError::ChecksumMismatch);
        }
        make_executable(file.path())?;
        Ok(file.into_temp_path())
    }
}

fn cache_is_fresh(state: &UpdateState) -> bool {
    let Ok(now) = unix_seconds() else {
        return false;
    };
    cache_is_fresh_at(state, now)
}

fn cache_is_fresh_at(state: &UpdateState, now: u64) -> bool {
    let Some(last_checked) = state.last_checked_unix_seconds else {
        return false;
    };
    now.saturating_sub(last_checked) < CHECK_INTERVAL.as_secs() && state.cached_release.is_some()
}

fn verify_candidate(candidate: &Path, version: &Version) -> Result<(), UpdateError> {
    let output = Command::new(candidate)
        .arg("--version")
        .output()
        .map_err(UpdateError::ExecuteCandidate)?;
    if !output.status.success() {
        return Err(UpdateError::CandidateRejected);
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let expected = version.to_string();
    if !text.split_whitespace().any(|part| part == expected) {
        return Err(UpdateError::CandidateVersionMismatch {
            expected: version.clone(),
            output: bounded_output(&text),
        });
    }
    Ok(())
}

fn validate_https_url(value: &str, purpose: &'static str) -> Result<(), UpdateError> {
    let url = Url::parse(value).map_err(|source| UpdateError::InvalidUrl { purpose, source })?;
    let local_http = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"));
    if url.scheme() != "https" && !local_http {
        return Err(UpdateError::InsecureUrl(purpose));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), UpdateError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(UpdateError::InvalidChecksum)
    }
}

fn constant_time_hex_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left.to_ascii_lowercase() ^ right.to_ascii_lowercase())
        })
        == 0
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn environment_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn unix_seconds() -> Result<u64, UpdateError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(UpdateError::SystemClock)
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

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(UpdateError::SetCandidatePermissions)
}

#[cfg(windows)]
fn make_executable(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

fn bounded_output(value: &str) -> String {
    value.chars().take(200).collect()
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

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const fn current_target() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const fn current_target() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
const fn current_target() -> &'static str {
    "aarch64-unknown-linux-gnu"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "musl"))]
const fn current_target() -> &'static str {
    "aarch64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
const fn current_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "musl"))]
const fn current_target() -> &'static str {
    "x86_64-unknown-linux-musl"
}

#[cfg(all(target_arch = "aarch64", target_os = "windows"))]
const fn current_target() -> &'static str {
    "aarch64-pc-windows-msvc"
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const fn current_target() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(not(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"),
    all(target_arch = "aarch64", target_os = "linux", target_env = "musl"),
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
    all(target_arch = "x86_64", target_os = "linux", target_env = "musl"),
    all(target_arch = "aarch64", target_os = "windows"),
    all(target_arch = "x86_64", target_os = "windows")
)))]
const fn current_target() -> &'static str {
    "unsupported"
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("this nan-harness build does not have an update channel configured")]
    UpdateChannelUnavailable,
    #[error("could not determine the NaN configuration directory")]
    MissingConfigDirectory,
    #[error("could not parse the running NaN version: {0}")]
    Version(semver::Error),
    #[error("could not build the update client: {0}")]
    BuildClient(reqwest::Error),
    #[error("the {purpose} URL is invalid: {source}")]
    InvalidUrl {
        purpose: &'static str,
        source: url::ParseError,
    },
    #[error("the {0} URL must use HTTPS")]
    InsecureUrl(&'static str),
    #[error("could not fetch update metadata: {0}")]
    FetchManifest(reqwest::Error),
    #[error("the update server returned HTTP {0} for the release manifest")]
    ManifestStatus(u16),
    #[error("the update manifest exceeds the 1 MiB safety limit")]
    ManifestTooLarge,
    #[error("the update manifest is not valid JSON: {0}")]
    ParseManifest(serde_json::Error),
    #[error("release manifest schema {0} is not supported")]
    UnsupportedManifestSchema(u8),
    #[error("the release manifest does not contain artifacts")]
    EmptyArtifactCatalog,
    #[error("the release manifest contains an invalid SHA-256 checksum")]
    InvalidChecksum,
    #[error("the release does not contain an artifact for target '{0}'")]
    MissingArtifact(String),
    #[error("could not download the update artifact: {0}")]
    DownloadArtifact(reqwest::Error),
    #[error("the update server returned HTTP {0} for the release artifact")]
    ArtifactStatus(u16),
    #[error("the update artifact exceeds the 128 MiB safety limit")]
    ArtifactTooLarge,
    #[error("could not create a temporary update artifact: {0}")]
    CreateCandidate(std::io::Error),
    #[error("could not write the temporary update artifact: {0}")]
    WriteCandidate(std::io::Error),
    #[error("could not make the temporary update artifact executable: {0}")]
    SetCandidatePermissions(std::io::Error),
    #[error("the downloaded update failed SHA-256 verification")]
    ChecksumMismatch,
    #[error("could not execute the downloaded update: {0}")]
    ExecuteCandidate(std::io::Error),
    #[error("the downloaded update did not pass its version check")]
    CandidateRejected,
    #[error("the downloaded update reported '{output}' instead of version {expected}")]
    CandidateVersionMismatch { expected: Version, output: String },
    #[error("could not replace the running nan-harness executable: {0}")]
    ReplaceExecutable(std::io::Error),
    #[error("could not remove the temporary update artifact: {0}")]
    RemoveCandidate(std::io::Error),
    #[error("could not create the NaN configuration directory: {0}")]
    CreateConfigDirectory(std::io::Error),
    #[error("could not read update settings: {0}")]
    ReadState(std::io::Error),
    #[error("update settings are not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("update settings schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not serialize update settings: {0}")]
    SerializeState(serde_json::Error),
    #[error("could not write update settings: {0}")]
    WriteState(std::io::Error),
    #[error("the system clock is before the Unix epoch: {0}")]
    SystemClock(std::time::SystemTimeError),
    #[error("could not read or write the update prompt: {0}")]
    Prompt(std::io::Error),
    #[error("could not restart NaN after updating: {0}")]
    Restart(std::io::Error),
}

impl UpdateError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UpdateChannelUnavailable
            | Self::MissingConfigDirectory
            | Self::ReadState(_)
            | Self::ParseState(_)
            | Self::UnsupportedStateSchema(_)
            | Self::CreateConfigDirectory(_)
            | Self::SerializeState(_)
            | Self::WriteState(_)
            | Self::SystemClock(_)
            | Self::Prompt(_) => "NH-UPDATE-001",
            Self::BuildClient(_)
            | Self::FetchManifest(_)
            | Self::ManifestStatus(_)
            | Self::ManifestTooLarge
            | Self::DownloadArtifact(_)
            | Self::ArtifactStatus(_) => "NH-UPDATE-002",
            Self::Version(_)
            | Self::InvalidUrl { .. }
            | Self::InsecureUrl(_)
            | Self::ParseManifest(_)
            | Self::UnsupportedManifestSchema(_)
            | Self::EmptyArtifactCatalog
            | Self::InvalidChecksum
            | Self::MissingArtifact(_) => "NH-UPDATE-003",
            Self::ArtifactTooLarge
            | Self::CreateCandidate(_)
            | Self::WriteCandidate(_)
            | Self::SetCandidatePermissions(_)
            | Self::ChecksumMismatch => "NH-UPDATE-004",
            Self::ExecuteCandidate(_)
            | Self::CandidateRejected
            | Self::CandidateVersionMismatch { .. } => "NH-UPDATE-005",
            Self::ReplaceExecutable(_) | Self::RemoveCandidate(_) | Self::Restart(_) => {
                "NH-UPDATE-006"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ReleaseArtifact, ReleaseManifest, UpdateManager, UpdateState, UpdateStateStore,
        cache_is_fresh_at, current_target,
    };
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use axum::routing::get;
    use semver::Version;
    use sha2::{Digest as _, Sha256};
    use std::sync::Arc;

    #[test]
    fn cached_update_results_expire_after_one_hour() {
        let checked_at = 10_000;
        let state = UpdateState {
            last_checked_unix_seconds: Some(checked_at),
            cached_release: Some(manifest("0.2.0", "https://example.com/nan")),
            ..UpdateState::default()
        };

        assert!(cache_is_fresh_at(&state, checked_at + 3_599));
        assert!(!cache_is_fresh_at(&state, checked_at + 3_600));
    }

    #[tokio::test]
    async fn skipped_release_returns_only_when_a_newer_version_exists() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manifest = manifest("0.2.0", "https://example.com/nan");
        let server = manifest_server(manifest.clone()).await;
        let manager = UpdateManager::new(
            "0.1.0",
            Some(format!("{server}/manifest.json")),
            UpdateStateStore::new(directory.path()),
        )
        .expect("manager should build");

        let available = manager
            .available_release(true, true)
            .await
            .expect("release should load")
            .expect("release should be newer");
        assert_eq!(available.version, Version::new(0, 2, 0));

        manager
            .skip(available.version.clone())
            .expect("skip should persist");
        assert!(
            manager
                .available_release(false, true)
                .await
                .expect("cached release should load")
                .is_none()
        );
        assert!(
            manager
                .available_release(false, false)
                .await
                .expect("manual check should load")
                .is_some()
        );
    }

    #[tokio::test]
    async fn current_release_is_not_offered() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let server = manifest_server(manifest("0.1.0", "https://example.com/nan")).await;
        let manager = UpdateManager::new(
            "0.1.0",
            Some(format!("{server}/manifest.json")),
            UpdateStateStore::new(directory.path()),
        )
        .expect("manager should build");

        assert!(
            manager
                .available_release(true, true)
                .await
                .expect("release should load")
                .is_none()
        );
    }

    #[tokio::test]
    async fn oversized_release_manifests_are_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let server = serve(Router::new().route(
            "/manifest.json",
            get(|| async { vec![b'x'; super::MAX_MANIFEST_SIZE + 1] }),
        ))
        .await;
        let manager = UpdateManager::new(
            "0.1.0",
            Some(format!("{server}/manifest.json")),
            UpdateStateStore::new(directory.path()),
        )
        .expect("manager should build");

        let error = manager
            .available_release(true, true)
            .await
            .expect_err("oversized manifests must be rejected");
        assert!(matches!(error, super::UpdateError::ManifestTooLarge));
    }

    #[test]
    fn manifests_require_secure_urls_and_valid_checksums() {
        let mut release = manifest("0.2.0", "https://example.com/nan");
        assert!(release.validate().is_ok());

        release.notes_url = "http://example.com/notes".to_owned();
        assert!(release.validate().is_err());
        release.notes_url = "https://example.com/notes".to_owned();
        release.artifacts[0].sha256 = "invalid".to_owned();
        assert!(release.validate().is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn downloads_and_verifies_an_executable_candidate() {
        use std::os::unix::fs::PermissionsExt as _;

        let binary = b"#!/bin/sh\nprintf '%s\\n' 'nan-harness 0.2.0'\n".to_vec();
        let checksum = super::hex_digest(Sha256::digest(&binary));
        let binary = Arc::new(binary);
        let binary_server = {
            let binary = Arc::clone(&binary);
            serve(Router::new().route(
                "/nan",
                get(move || {
                    let binary = Arc::clone(&binary);
                    async move {
                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::from(binary.as_ref().clone()))
                            .expect("response should build")
                    }
                }),
            ))
            .await
        };
        let release = ReleaseManifest {
            schema_version: 1,
            version: Version::new(0, 2, 0),
            notes_url: "https://example.com/notes".to_owned(),
            artifacts: vec![ReleaseArtifact {
                target: current_target().to_owned(),
                url: format!("{binary_server}/nan"),
                sha256: checksum,
            }],
        };
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manager = UpdateManager::new(
            "0.1.0",
            Some("https://example.com/manifest.json".to_owned()),
            UpdateStateStore::new(directory.path()),
        )
        .expect("manager should build");

        let candidate = manager
            .download(&release.artifacts[0])
            .await
            .expect("candidate should download");
        let candidate_path: &std::path::Path = candidate.as_ref();
        assert_eq!(
            std::fs::metadata(candidate_path)
                .expect("metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        super::verify_candidate(candidate_path, &release.version)
            .expect("candidate should report the expected version");
    }

    fn manifest(version: &str, artifact_url: &str) -> ReleaseManifest {
        ReleaseManifest {
            schema_version: 1,
            version: Version::parse(version).expect("version should parse"),
            notes_url: "https://example.com/notes".to_owned(),
            artifacts: vec![ReleaseArtifact {
                target: current_target().to_owned(),
                url: artifact_url.to_owned(),
                sha256: "0".repeat(64),
            }],
        }
    }

    async fn manifest_server(manifest: ReleaseManifest) -> String {
        serve(Router::new().route(
            "/manifest.json",
            get(move || {
                let manifest = manifest.clone();
                async move { axum::Json(manifest) }
            }),
        ))
        .await
    }

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("address should resolve");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("server should run");
        });
        format!("http://{address}")
    }
}
