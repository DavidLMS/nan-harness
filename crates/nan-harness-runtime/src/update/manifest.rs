use super::UpdateError;
use crate::update::candidate::validate_sha256;
use futures_util::StreamExt as _;
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

const RELEASE_MANIFEST_SCHEMA_VERSION: u8 = 1;
pub(super) const MAX_MANIFEST_SIZE: usize = 1024 * 1024;

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
    pub(super) fn validate(&self) -> Result<(), UpdateError> {
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

    pub(super) fn artifact_for_current_target(&self) -> Result<&ReleaseArtifact, UpdateError> {
        let target = current_target();
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
            .ok_or_else(|| UpdateError::MissingArtifact(target.to_owned()))
    }
}

pub(super) async fn fetch_release(
    client: &reqwest::Client,
    url: &str,
) -> Result<ReleaseManifest, UpdateError> {
    let response = client
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
    let release =
        serde_json::from_slice::<ReleaseManifest>(&contents).map_err(UpdateError::ParseManifest)?;
    release.validate()?;
    Ok(release)
}

pub(super) fn validate_https_url(value: &str, purpose: &'static str) -> Result<(), UpdateError> {
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

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub(super) const fn current_target() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
pub(super) const fn current_target() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
pub(super) const fn current_target() -> &'static str {
    "aarch64-unknown-linux-gnu"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "musl"))]
pub(super) const fn current_target() -> &'static str {
    "aarch64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
pub(super) const fn current_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "musl"))]
pub(super) const fn current_target() -> &'static str {
    "x86_64-unknown-linux-musl"
}

#[cfg(all(target_arch = "aarch64", target_os = "windows"))]
pub(super) const fn current_target() -> &'static str {
    "aarch64-pc-windows-msvc"
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
pub(super) const fn current_target() -> &'static str {
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
pub(super) const fn current_target() -> &'static str {
    "unsupported"
}

#[cfg(test)]
mod tests {
    use crate::update::tests::manifest;

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
}
