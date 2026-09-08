//! Download and unpack official applications into journal-owned directories only.

mod archive;
mod dmg;
mod process;

use crate::catalog::{self, Distribution, Installation, PackageFormat};
use crate::journal::{Journal, JournalError};
use crate::report::{Architecture, Platform};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("the official distribution requires an external installation step")]
    ExternalInstallation,
    #[error("no official distribution is available for this platform")]
    Unavailable,
    #[error("the application download failed")]
    Download,
    #[error("the application archive exceeds the installation limit")]
    TooLarge,
    #[error("the application archive is invalid or unsafe")]
    Archive,
    #[error("the platform extraction tool is unavailable or failed")]
    Extraction,
    #[error("the read-only installer volume may still be mounted; recovery was preserved")]
    MountPending,
    #[error("the private installation could not be accessed")]
    Io(#[from] std::io::Error),
    #[error("the private installation ownership could not be recorded")]
    Journal(#[from] JournalError),
    #[error("the unpacked application could not be identified")]
    Discovery(#[from] catalog::DiscoveryError),
}

/// Install without touching package databases, PATH, or an existing application.
///
/// # Errors
/// Fails closed on unavailable distributions, unsafe archives or failed ownership.
/// Store/setup/source-build distributions require a separately qualified install flow.
pub async fn install(
    kind: DesktopHarnessKind,
    journal: &mut Journal,
) -> Result<Installation, InstallError> {
    let distribution = catalog::download(kind, Platform::current(), Architecture::current());
    let (url, format) = match distribution {
        Distribution::Direct { url, format } if format != PackageFormat::WindowsSetup => {
            (url, format)
        }
        Distribution::Unavailable { .. } => return Err(InstallError::Unavailable),
        _ => return Err(InstallError::ExternalInstallation),
    };
    let name = format!("install-{kind}");
    let root = journal.reserve(&name)?;
    let result = install_owned(kind, &url, format, &root).await;
    // Even failed partial downloads belong to this run. Snapshot them before
    // returning so routine cleanup need not mistake them for crash-time changes.
    if !matches!(result, Err(InstallError::MountPending)) {
        journal.seal(&name)?;
    }
    result
}

async fn install_owned(
    kind: DesktopHarnessKind,
    url: &str,
    format: PackageFormat,
    root: &Path,
) -> Result<Installation, InstallError> {
    let download = root.join("download");
    download_file(url, &download, MAX_DOWNLOAD_BYTES).await?;
    let destination = root.join("application");
    nan_harness_private_fs::create_private_dir(&destination)?;
    let executable = match format {
        PackageFormat::Dmg => dmg::extract(&download, &destination, kind).await?,
        PackageFormat::TarGz => {
            archive::extract_gzip(&download, &destination)?;
            find_executable(&destination, kind)?
        }
        PackageFormat::Deb => {
            archive::extract_deb(&download, &destination, root).await?;
            find_executable(&destination, kind)?
        }
        PackageFormat::WindowsSetup => return Err(InstallError::ExternalInstallation),
    };
    catalog::inspect(kind, &executable).map_err(InstallError::from)
}

/// Stream a bounded HTTPS response to a newly created private file.
///
/// # Errors
/// Rejects credentials in URLs, insecure redirects, unsuccessful HTTP responses,
/// excessive size/time and existing destinations. Never returns response bodies.
pub async fn download_file(url: &str, path: &Path, max_bytes: u64) -> Result<(), InstallError> {
    let url = url::Url::parse(url).map_err(|_| InstallError::Download)?;
    if !safe_download_url(&url) {
        return Err(InstallError::Download);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_mins(5))
        .user_agent("nan-harness-desktop-check")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 8 || !safe_download_url(attempt.url()) {
                attempt.error("unsafe download redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| InstallError::Download)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| InstallError::Download)?;
    if !response.status().is_success() {
        return Err(InstallError::Download);
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(InstallError::TooLarge);
    }
    let mut file = open_private_new(path)?;
    let mut received = 0u64;
    while let Some(bytes) = response.chunk().await.map_err(|_| InstallError::Download)? {
        received = received
            .checked_add(bytes.len() as u64)
            .ok_or(InstallError::TooLarge)?;
        if received > max_bytes {
            return Err(InstallError::TooLarge);
        }
        file.write_all(&bytes)?;
    }
    if received == 0 {
        return Err(InstallError::Download);
    }
    file.sync_all()?;
    Ok(())
}

fn safe_download_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn find_executable(root: &Path, kind: DesktopHarnessKind) -> Result<PathBuf, InstallError> {
    let expected = match kind {
        DesktopHarnessKind::ChatGpt => &["usr/lib/chatgpt/ChatGPT"][..],
        DesktopHarnessKind::Zed => &["zed.app/bin/zed"][..],
        DesktopHarnessKind::Pen => &["Pen", "pen", "Pen-linux-x64/Pen", "Pen-linux-arm64/Pen"][..],
        _ => &[],
    };
    let mut found = Vec::new();
    for relative in expected {
        let path = root.join(relative);
        if let Ok(metadata) = std::fs::symlink_metadata(&path)
            && metadata.is_file()
        {
            found.push(path);
        }
    }
    if kind == DesktopHarnessKind::Pen {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && entry.file_name().to_string_lossy().starts_with("Pen-")
            {
                for name in ["Pen", "pen"] {
                    let path = entry.path().join(name);
                    if let Ok(metadata) = std::fs::symlink_metadata(&path)
                        && metadata.is_file()
                    {
                        found.push(path);
                    }
                }
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(InstallError::Archive),
        _ => Err(InstallError::Discovery(catalog::DiscoveryError::Ambiguous)),
    }
}

#[cfg(test)]
mod tests {
    use super::safe_download_url;

    #[test]
    fn download_sources_do_not_accept_credentials_or_insecure_schemes() {
        for rejected in [
            "http://example.com/app",
            "https://user:secret@example.com/app",
            "file:///app",
            "https://example.com/app#fragment",
        ] {
            assert!(!safe_download_url(
                &url::Url::parse(rejected).expect("fixture URL")
            ));
        }
        assert!(safe_download_url(
            &url::Url::parse("https://github.com/owner/repo/releases/download/v1/app")
                .expect("fixture URL")
        ));
    }
}
