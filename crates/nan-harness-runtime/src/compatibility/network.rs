use super::validation::validate_manifest;
use super::{CompatibilityError, VerificationManifest};
use futures_util::StreamExt as _;
use nan_harness_core::CompatibilityManifest;
use std::time::Duration;
use url::Url;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_REDIRECTS: usize = 3;
pub(super) const MAX_MANIFEST_SIZE: usize = 1024 * 1024;

pub(super) async fn fetch_manifest(
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

pub(super) fn redirect_is_allowed(initial_url: &Url, next_url: &Url) -> bool {
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
