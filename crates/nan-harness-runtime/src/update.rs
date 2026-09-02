mod artifact;
mod candidate;
mod manifest;
mod replacement;
mod state;

use semver::Version;
use std::env;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

use candidate::verify_candidate;
use manifest::{fetch_release, validate_https_url};
use replacement::replace_running_executable;
use state::{UpdateStateStore, cache_is_fresh, unix_seconds};

pub use manifest::{ReleaseArtifact, ReleaseManifest};

pub const UPDATE_MANIFEST_ENVIRONMENT_VARIABLE: &str = "NAN_UPDATE_MANIFEST_URL";
pub const DISABLE_UPDATE_CHECK_ENVIRONMENT_VARIABLE: &str = "NAN_NO_UPDATE_CHECK";
pub const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const BUILD_UPDATE_MANIFEST_URL: Option<&str> = option_env!("NAN_UPDATE_MANIFEST_URL");

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
    /// skipped. Cached metadata is reused for one hour, while a deferred release remains visible
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
        let Some(manifest_url) = self.manifest_url.as_deref() else {
            return Err(UpdateError::UpdateChannelUnavailable);
        };
        let mut state = self.state.load()?;
        let release = if !force_refresh && cache_is_fresh(&state) {
            state.cached_release.clone()
        } else {
            let release = fetch_release(&self.client, manifest_url).await?;
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
        let mut state = self.state.load()?;
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
        let candidate = artifact::download(&self.client, artifact).await?;
        let candidate_path: &Path = candidate.as_ref();
        verify_candidate(candidate_path, &release.version)?;
        replace_running_executable(candidate_path).map_err(UpdateError::ReplaceExecutable)?;
        candidate.close().map_err(UpdateError::RemoveCandidate)
    }
}

fn environment_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("this nan-harness build does not have an update channel configured")]
    UpdateChannelUnavailable,
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not parse the running nan-harness version: {0}")]
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
    #[error("could not create the nan-harness configuration directory: {0}")]
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
    #[error("could not restart nan-harness after updating: {0}")]
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
mod tests;
