//! Download and unpack frozen official applications into journal-owned directories only.

mod archive;
mod dmg;
mod process;

use crate::catalog::frozen::{self, Entry, Installer, PackageFormat, Release};
use crate::catalog::{self, Installation};
use crate::journal::{Journal, JournalError};
use crate::report::{Architecture, Platform};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
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
    #[error("the application bytes do not match their frozen digest")]
    DigestMismatch,
    #[error("the installed application does not match its frozen version")]
    VersionMismatch,
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

/// Closed, non-sensitive failures at the bounded HTTPS fetch boundary.
#[derive(Debug)]
pub enum FetchFailure {
    InvalidUrl,
    Policy,
    ClientSetup,
    Timeout,
    Connect,
    HttpStatus(u16),
    Request,
    BodyRead,
    BodyBound,
    LocalIo(std::io::Error),
}

/// Resolve the official latest release once, then install exactly that frozen release.
///
/// # Errors
/// Fails closed on unavailable distributions, unsafe archives or failed ownership.
/// Store/setup/source-build distributions require a separately qualified install flow.
pub async fn install(
    kind: DesktopHarnessKind,
    journal: &mut Journal,
) -> Result<Installation, InstallError> {
    let name = format!("resolve-{kind}");
    let artifacts = journal.reserve(&name)?;
    let resolved = frozen::resolve_entry(
        kind,
        Platform::current(),
        Architecture::current(),
        &artifacts,
        &mut frozen::OfficialFetch,
    )
    .await;
    let result = match resolved {
        Err(_) => Err(InstallError::MountPending),
        Ok(Entry::Blocked(_)) => Err(InstallError::Unavailable),
        Ok(Entry::Frozen(release)) => install_frozen(&release, Some(&artifacts), journal).await,
    };
    if !matches!(result, Err(InstallError::MountPending)) {
        journal.seal(&name)?;
    }
    result
}

/// Install one frozen release; never consults a moving latest source.
///
/// # Errors
/// Rejects external installers, digest drift, unsafe archives and version drift.
pub async fn install_frozen(
    release: &Release,
    artifacts: Option<&Path>,
    journal: &mut Journal,
) -> Result<Installation, InstallError> {
    if release.installer != Installer::Checker {
        return Err(InstallError::ExternalInstallation);
    }
    let name = format!("install-{}", release.app);
    let root = journal.reserve(&name)?;
    let result = install_owned(release, artifacts, &root).await;
    // Even failed partial downloads belong to this run. Snapshot them before
    // returning so routine cleanup need not mistake them for crash-time changes.
    if !matches!(result, Err(InstallError::MountPending)) {
        journal.seal(&name)?;
    }
    result
}

async fn install_owned(
    release: &Release,
    artifacts: Option<&Path>,
    root: &Path,
) -> Result<Installation, InstallError> {
    let kind = release.app;
    let download = root.join("download");
    let expected = release
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or(InstallError::DigestMismatch)?;
    let actual = if release.staged {
        let staged = artifacts
            .and_then(|artifacts| frozen::staged_path(artifacts, release))
            .ok_or(InstallError::Unavailable)?;
        // Hash the private copy that is extracted, not the caller-owned staged file.
        copy_hashed(&staged, &download)?
    } else {
        download_file(&release.url, &download, MAX_DOWNLOAD_BYTES).await?;
        sha256_file(&download)?
    };
    if actual != expected {
        return Err(InstallError::DigestMismatch);
    }
    let destination = root.join("application");
    nan_harness_private_fs::create_private_dir(&destination)?;
    let executable = match release.format {
        PackageFormat::Dmg => dmg::extract(&download, &destination, kind).await?,
        PackageFormat::Zip => dmg::extract_zip(&download, root, &destination, kind).await?,
        PackageFormat::TarGz => {
            archive::extract_gzip(&download, &destination)?;
            find_executable(&destination, kind)?
        }
        PackageFormat::Deb => {
            archive::extract_deb(&download, &destination, root).await?;
            find_executable(&destination, kind)?
        }
        PackageFormat::Msix | PackageFormat::WindowsSetup | PackageFormat::Source => {
            return Err(InstallError::ExternalInstallation);
        }
    };
    let installation = catalog::inspect(kind, &executable)?;
    verify_version(release, &installation)?;
    Ok(installation)
}

/// The installed application must report exactly the frozen three-component version.
///
/// # Errors
/// Returns `VersionMismatch` for unknown or different versions.
pub fn verify_version(release: &Release, installation: &Installation) -> Result<(), InstallError> {
    let expected = frozen::exact_version(&release.version).ok_or(InstallError::VersionMismatch)?;
    if installation.app_version.as_ref() != Some(&expected) {
        return Err(InstallError::VersionMismatch);
    }
    if let Some(runtime) = &release.runtime_version
        && installation.runtime_version.as_ref() != frozen::exact_version(runtime).as_ref()
    {
        return Err(InstallError::VersionMismatch);
    }
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    copy_hashed_into(&mut std::fs::File::open(path)?, &mut std::io::sink())
}

fn copy_hashed(source: &Path, destination: &Path) -> Result<String, InstallError> {
    let input = std::fs::File::open(source).map_err(|_| InstallError::Unavailable)?;
    if input.metadata()?.len() > MAX_DOWNLOAD_BYTES {
        return Err(InstallError::TooLarge);
    }
    let mut output = open_private_new(destination)?;
    let digest = copy_hashed_into(&mut input.take(MAX_DOWNLOAD_BYTES + 1), &mut output)?;
    output.sync_all()?;
    Ok(digest)
}

fn copy_hashed_into(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
) -> std::io::Result<String> {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        }))
}

fn client() -> Result<reqwest::Client, FetchFailure> {
    reqwest::Client::builder()
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
        .map_err(|_| FetchFailure::ClientSetup)
}

async fn response(url: &str, max_bytes: u64) -> Result<reqwest::Response, FetchFailure> {
    let url = url::Url::parse(url).map_err(|_| FetchFailure::InvalidUrl)?;
    if !safe_download_url(&url) {
        return Err(FetchFailure::Policy);
    }
    let response = client()
        .map_err(|_| FetchFailure::ClientSetup)?
        .get(url)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                FetchFailure::Timeout
            } else if error.is_connect() {
                FetchFailure::Connect
            } else {
                FetchFailure::Request
            }
        })?;
    validate_response(response, max_bytes)
}

fn validate_response(
    response: reqwest::Response,
    max_bytes: u64,
) -> Result<reqwest::Response, FetchFailure> {
    if !response.status().is_success() {
        return Err(FetchFailure::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(FetchFailure::BodyBound);
    }
    Ok(response)
}

pub(crate) async fn fetch_bounded_detailed(
    url: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, FetchFailure> {
    read_response(response(url, max_bytes).await?, max_bytes).await
}

async fn read_response(
    mut response: reqwest::Response,
    max_bytes: u64,
) -> Result<Vec<u8>, FetchFailure> {
    let mut body = Vec::new();
    while let Some(bytes) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            FetchFailure::Timeout
        } else if error.is_connect() {
            FetchFailure::Connect
        } else {
            FetchFailure::BodyRead
        }
    })? {
        if body.len() as u64 + bytes.len() as u64 > max_bytes {
            return Err(FetchFailure::BodyBound);
        }
        body.extend_from_slice(&bytes);
    }
    Ok(body)
}

/// Stream a bounded HTTPS response to a newly created private file.
///
/// # Errors
/// Rejects credentials in URLs, insecure redirects, unsuccessful HTTP responses,
/// excessive size/time and existing destinations. Never returns response bodies.
pub async fn download_file(url: &str, path: &Path, max_bytes: u64) -> Result<(), InstallError> {
    download_file_detailed(url, path, max_bytes)
        .await
        .map_err(install_error)
}

pub(crate) async fn download_file_detailed(
    url: &str,
    path: &Path,
    max_bytes: u64,
) -> Result<(), FetchFailure> {
    let mut response = response(url, max_bytes).await?;
    let mut file = open_private_new(path).map_err(FetchFailure::LocalIo)?;
    let mut received = 0u64;
    while let Some(bytes) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            FetchFailure::Timeout
        } else if error.is_connect() {
            FetchFailure::Connect
        } else {
            FetchFailure::BodyRead
        }
    })? {
        received = received
            .checked_add(bytes.len() as u64)
            .ok_or(FetchFailure::BodyBound)?;
        if received > max_bytes {
            return Err(FetchFailure::BodyBound);
        }
        file.write_all(&bytes).map_err(FetchFailure::LocalIo)?;
    }
    if received == 0 {
        return Err(FetchFailure::BodyBound);
    }
    file.sync_all().map_err(FetchFailure::LocalIo)?;
    Ok(())
}

fn install_error(error: FetchFailure) -> InstallError {
    match error {
        FetchFailure::BodyBound => InstallError::TooLarge,
        FetchFailure::LocalIo(error) => InstallError::Io(error),
        _ => InstallError::Download,
    }
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
        DesktopHarnessKind::Claude => &["usr/lib/claude-desktop/claude-desktop"][..],
        DesktopHarnessKind::Pen => &["Pen", "pen", "Pen-linux-x64/Pen", "Pen-linux-arm64/Pen"][..],
        DesktopHarnessKind::Hermes => &[],
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
                        && !found.contains(&path)
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
    use super::*;

    async fn synthetic_response(wire: &'static [u8], stall: bool) -> reqwest::Response {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream.write_all(wire).await.unwrap();
            if stall {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        });
        reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(150))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn response_boundaries_preserve_status_size_body_and_timeout_facts() {
        let response = synthetic_response(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n",
            false,
        )
        .await;
        assert!(matches!(
            validate_response(response, 10),
            Err(FetchFailure::HttpStatus(403))
        ));
        let response =
            synthetic_response(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n", false).await;
        assert!(matches!(
            validate_response(response, 10),
            Err(FetchFailure::BodyBound)
        ));
        let response = synthetic_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nABCD\r\n0\r\n\r\n",
            false,
        )
        .await;
        assert!(matches!(
            read_response(validate_response(response, 3).unwrap(), 3).await,
            Err(FetchFailure::BodyBound)
        ));
        let response =
            synthetic_response(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nA", false).await;
        assert!(matches!(
            read_response(validate_response(response, 10).unwrap(), 10).await,
            Err(FetchFailure::BodyRead)
        ));
        let response =
            synthetic_response(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n", true).await;
        assert!(matches!(
            read_response(validate_response(response, 10).unwrap(), 10).await,
            Err(FetchFailure::Timeout)
        ));
        assert!(matches!(
            super::response("http://127.0.0.1", 10).await,
            Err(FetchFailure::Policy)
        ));
    }

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

    #[test]
    fn bounded_fetch_failures_keep_install_size_and_local_io_boundaries() {
        assert!(matches!(
            install_error(FetchFailure::BodyBound),
            InstallError::TooLarge
        ));
        assert!(matches!(
            install_error(FetchFailure::LocalIo(std::io::Error::other("synthetic"))),
            InstallError::Io(_)
        ));
        assert!(matches!(
            install_error(FetchFailure::Connect),
            InstallError::Download
        ));
    }

    #[test]
    fn staged_bytes_are_hashed_as_copied() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("staged");
        std::fs::write(&source, b"abc").unwrap();
        let digest = copy_hashed(&source, &directory.path().join("copy")).unwrap();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            std::fs::read(directory.path().join("copy")).unwrap(),
            b"abc"
        );
        assert!(copy_hashed(&source, &directory.path().join("copy")).is_err());
    }

    fn release(format: PackageFormat, staged: bool) -> Release {
        Release {
            app: DesktopHarnessKind::Pen,
            version: "1.2.3".into(),
            runtime_version: None,
            channel: String::new(),
            url: "https://www.pen.dev/download/Pen-linux-x64.tar.gz".into(),
            format,
            digest: Some(format!("sha256:{}", "0".repeat(64))),
            revision: None,
            staged,
            installer: Installer::Checker,
        }
    }

    #[tokio::test]
    async fn tampered_staged_bytes_are_rejected_before_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let artifacts = directory.path().join("artifacts");
        std::fs::create_dir(&artifacts).unwrap();
        let frozen = release(PackageFormat::TarGz, true);
        std::fs::write(
            frozen::staged_path(&artifacts, &frozen).unwrap(),
            b"tampered",
        )
        .unwrap();
        let root = directory.path().join("root");
        std::fs::create_dir(&root).unwrap();
        assert!(matches!(
            install_owned(&frozen, Some(&artifacts), &root).await,
            Err(InstallError::DigestMismatch)
        ));
        assert!(!root.join("application").exists());
        assert!(matches!(
            install_owned(&frozen, None, &directory.path().join("absent")).await,
            Err(InstallError::Unavailable)
        ));
    }

    #[test]
    fn installed_versions_must_equal_the_frozen_release() {
        let frozen = release(PackageFormat::TarGz, true);
        let installed = |version: Option<&str>| Installation {
            executable: PathBuf::from("/synthetic"),
            app_version: version.map(|version| version.parse().unwrap()),
            runtime_version: None,
        };
        assert!(verify_version(&frozen, &installed(Some("1.2.3"))).is_ok());
        for drift in [None, Some("1.2.4"), Some("1.2.3-beta.1")] {
            assert!(matches!(
                verify_version(&frozen, &installed(drift)),
                Err(InstallError::VersionMismatch)
            ));
        }
        let mut with_runtime = frozen;
        with_runtime.runtime_version = Some("0.9.0".into());
        assert!(verify_version(&with_runtime, &installed(Some("1.2.3"))).is_err());
    }
}
