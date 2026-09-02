use super::candidate::{CandidateDigest, make_executable};
use super::{ReleaseArtifact, UpdateError};
use futures_util::StreamExt as _;
use std::io::Write as _;
use tempfile::{Builder as TempFileBuilder, TempPath};

const MAX_BINARY_SIZE: u64 = 128 * 1024 * 1024;

pub(super) async fn download(
    client: &reqwest::Client,
    artifact: &ReleaseArtifact,
) -> Result<TempPath, UpdateError> {
    let response = client
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
    let mut digest = CandidateDigest::new();
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
    if !digest.matches(&artifact.sha256) {
        return Err(UpdateError::ChecksumMismatch);
    }
    make_executable(file.path())?;
    Ok(file.into_temp_path())
}
