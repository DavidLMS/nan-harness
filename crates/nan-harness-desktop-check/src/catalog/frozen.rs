//! Frozen official Desktop releases: resolve once, then install only these exact inputs.
//!
//! A manifest is private run state. It may contain official download URLs and
//! staged-artifact digests, so it is never uploaded or embedded in public reports.

pub(crate) mod inspect;
mod resolve;
#[cfg(test)]
mod tests;

pub use inspect::InspectError;
pub use resolve::{Fetch, OfficialFetch, ResolveError, resolve, resolve_entry};

use crate::report::{Architecture, Platform};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u8 = 1;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageFormat {
    Dmg,
    Zip,
    TarGz,
    Deb,
    Msix,
    WindowsSetup,
    Source,
}

/// Who changes the system: the checker unpacks into journal-owned directories;
/// external installers are disposable-runner steps that consume the same entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Installer {
    Checker,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockReason {
    /// The publisher documents that this platform is not offered.
    UpstreamUnsupported,
    /// No official artifact exists for this native target in the qualified catalog.
    UnqualifiedPlatform,
    /// Official metadata or artifact inspection did not yield one exact version.
    ResolutionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u8,
    pub suite: String,
    pub platform: Platform,
    pub architecture: Architecture,
    pub model: String,
    pub apps: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Entry {
    Frozen(Release),
    Blocked(Blocker),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub app: DesktopHarnessKind,
    /// Exact three-component application version from official metadata or the frozen bytes.
    pub version: String,
    /// Present only when read from the same frozen artifact; never inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    pub channel: String,
    pub url: String,
    pub format: PackageFormat,
    /// `sha256:<hex>` for every downloadable artifact; absent only for source revisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Bytes were downloaded during resolution and must be installed from the staged copy.
    pub staged: bool,
    pub installer: Installer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blocker {
    pub app: DesktopHarnessKind,
    pub reason: BlockReason,
    pub evidence: String,
}

impl Entry {
    #[must_use]
    pub const fn app(&self) -> DesktopHarnessKind {
        match self {
            Self::Frozen(release) => release.app,
            Self::Blocked(blocker) => blocker.app,
        }
    }
}

/// Closed, allowlisted official source for one app and native target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Publisher APT index with per-package SHA-256; no APT source is registered.
    Apt {
        base: &'static str,
        package: &'static str,
        architecture: &'static str,
    },
    /// Sparkle appcast naming an immutable versioned archive.
    Sparkle {
        appcast: &'static str,
        archive_prefix: &'static str,
        hardware: &'static str,
    },
    /// Squirrel.Mac release JSON naming an immutable versioned archive.
    SquirrelMac {
        releases: &'static str,
        archive_prefix: &'static str,
    },
    /// GitHub latest non-prerelease with a publisher asset digest.
    GithubAsset {
        repository: &'static str,
        asset: &'static str,
        format: PackageFormat,
        installer: Installer,
    },
    /// Official source at the latest release tag's exact commit.
    GithubSource {
        repository: &'static str,
        package_json: &'static str,
    },
    /// A moving official URL: the bytes themselves are frozen and inspected.
    Moving {
        url: &'static str,
        format: PackageFormat,
        installer: Installer,
    },
    Blocked {
        reason: BlockReason,
        evidence: &'static str,
    },
}

const CHATGPT_DEB: &str = "https://persistent.oaistatic.com/codex-app-prod/linux/deb/";
const CLAUDE_DEB: &str = "https://downloads.claude.ai/claude-desktop/apt/stable/";

/// Official sources checked 2026-09-12; see `canary/checker-platforms.md`.
#[must_use]
pub(crate) const fn policy(
    kind: DesktopHarnessKind,
    platform: Platform,
    architecture: Architecture,
) -> Policy {
    use Architecture::{Aarch64, X86_64};
    use DesktopHarnessKind::{ChatGpt, Claude, Hermes, Pen, Zed};
    use Platform::{Linux, Macos, Windows};
    let debian = match architecture {
        X86_64 => "amd64",
        Aarch64 => "arm64",
    };
    match (kind, platform, architecture) {
        (ChatGpt, Linux, _) => Policy::Apt {
            base: CHATGPT_DEB,
            package: "chatgpt",
            architecture: debian,
        },
        (ChatGpt, Macos, Aarch64) => Policy::Sparkle {
            appcast: "https://persistent.oaistatic.com/codex-app-prod/appcast.xml",
            archive_prefix: "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-arm64-",
            hardware: "arm64",
        },
        (ChatGpt, Macos, X86_64) => Policy::Blocked {
            reason: BlockReason::UpstreamUnsupported,
            evidence: "https://learn.chatgpt.com/docs/app",
        },
        (ChatGpt, Windows, X86_64) => Policy::Moving {
            url: "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix",
            format: PackageFormat::Msix,
            installer: Installer::External,
        },
        (ChatGpt, Windows, Aarch64) => Policy::Blocked {
            reason: BlockReason::UnqualifiedPlatform,
            evidence: "https://learn.chatgpt.com/docs/windows/windows-app",
        },
        (Claude, Linux, _) => Policy::Apt {
            base: CLAUDE_DEB,
            package: "claude-desktop",
            architecture: debian,
        },
        (Claude, Macos, _) => Policy::SquirrelMac {
            releases: "https://downloads.claude.ai/releases/darwin/universal/RELEASES.json",
            archive_prefix: "https://downloads.claude.ai/releases/darwin/universal/",
        },
        (Claude, Windows, X86_64) => Policy::Moving {
            url: "https://claude.ai/api/desktop/win32/x64/msix",
            format: PackageFormat::Msix,
            installer: Installer::External,
        },
        (Claude, Windows, Aarch64) => Policy::Blocked {
            reason: BlockReason::UnqualifiedPlatform,
            evidence: "https://claude.com/download",
        },
        (Hermes, _, _) => Policy::GithubSource {
            repository: "NousResearch/hermes-agent",
            package_json: "apps/desktop/package.json",
        },
        (Pen, platform, architecture) => pen_policy(platform, architecture),
        (Zed, Macos, X86_64) => zed("Zed-x86_64.dmg", PackageFormat::Dmg, Installer::Checker),
        (Zed, Macos, Aarch64) => zed("Zed-aarch64.dmg", PackageFormat::Dmg, Installer::Checker),
        (Zed, Linux, X86_64) => zed(
            "zed-linux-x86_64.tar.gz",
            PackageFormat::TarGz,
            Installer::Checker,
        ),
        (Zed, Linux, Aarch64) => zed(
            "zed-linux-aarch64.tar.gz",
            PackageFormat::TarGz,
            Installer::Checker,
        ),
        (Zed, Windows, X86_64) => zed(
            "Zed-x86_64.exe",
            PackageFormat::WindowsSetup,
            Installer::External,
        ),
        (Zed, Windows, Aarch64) => zed(
            "Zed-aarch64.exe",
            PackageFormat::WindowsSetup,
            Installer::External,
        ),
    }
}

/// Pen exposes moving download endpoints rather than an independent release index.
/// Keep its complete native artifact matrix together for inspection and staging.
const fn pen_policy(platform: Platform, architecture: Architecture) -> Policy {
    use Architecture::{Aarch64, X86_64};
    use Platform::{Linux, Macos, Windows};
    match (platform, architecture) {
        (Macos, Aarch64) => Policy::Moving {
            url: "https://www.pen.dev/download/Pen-mac-arm64.dmg",
            format: PackageFormat::Dmg,
            installer: Installer::Checker,
        },
        (Macos, X86_64) => Policy::Moving {
            url: "https://www.pen.dev/download/Pen-mac-x64.dmg",
            format: PackageFormat::Dmg,
            installer: Installer::Checker,
        },
        (Linux, X86_64) => Policy::Moving {
            url: "https://www.pen.dev/download/Pen-linux-x64.tar.gz",
            format: PackageFormat::TarGz,
            installer: Installer::Checker,
        },
        (Linux, Aarch64) => Policy::Moving {
            url: "https://www.pen.dev/download/Pen-linux-arm64.tar.gz",
            format: PackageFormat::TarGz,
            installer: Installer::Checker,
        },
        (Windows, X86_64) => Policy::Moving {
            url: "https://www.pen.dev/download/Pen-win-x64.exe",
            format: PackageFormat::WindowsSetup,
            installer: Installer::External,
        },
        (Windows, Aarch64) => Policy::Blocked {
            reason: BlockReason::UpstreamUnsupported,
            evidence: "https://www.pen.dev/downloads",
        },
    }
}

const fn zed(asset: &'static str, format: PackageFormat, installer: Installer) -> Policy {
    Policy::GithubAsset {
        repository: "zed-industries/zed",
        asset,
        format,
        installer,
    }
}

impl Policy {
    /// The stable channel identity recorded in a manifest; evidence for resolution failures.
    pub(crate) fn channel(self) -> String {
        match self {
            Self::Apt { base, .. } => format!("apt:{base}"),
            Self::Sparkle { appcast, .. } => format!("sparkle:{appcast}"),
            Self::SquirrelMac { releases, .. } => format!("squirrel-mac:{releases}"),
            Self::GithubAsset { repository, .. } => format!("github-release:{repository}"),
            Self::GithubSource { repository, .. } => format!("github-source:{repository}"),
            Self::Moving { url, .. } => format!("official-latest:{url}"),
            Self::Blocked { evidence, .. } => format!("blocked:{evidence}"),
        }
    }

    /// Public documentation or metadata location cited by a closed failure.
    pub(crate) fn evidence(self) -> String {
        match self {
            Self::Apt { base, .. } => base.to_owned(),
            Self::Sparkle { appcast, .. } => appcast.to_owned(),
            Self::SquirrelMac { releases, .. } => releases.to_owned(),
            Self::GithubAsset { repository, .. } | Self::GithubSource { repository, .. } => {
                format!("https://github.com/{repository}/releases/latest")
            }
            Self::Moving { url, .. } => url.to_owned(),
            Self::Blocked { evidence, .. } => evidence.to_owned(),
        }
    }
}

/// Accept only canonical `MAJOR.MINOR.PATCH`; four-component or prerelease values are not guessed.
#[must_use]
pub fn exact_version(text: &str) -> Option<Version> {
    let version = Version::parse(text).ok()?;
    (version.pre.is_empty() && version.build.is_empty() && version.to_string() == text)
        .then_some(version)
}

pub(crate) fn sha256_digest(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn sha256_hex(text: &str) -> bool {
    lower_hex(text, 64)
}

pub(crate) fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && model.as_bytes()[0].is_ascii_alphanumeric()
        && model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:/".contains(&byte))
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the frozen Desktop manifest cannot be read")]
    Unreadable,
    #[error("the frozen Desktop manifest is invalid")]
    Invalid,
    #[error("the frozen Desktop manifest does not match this platform, model or app selection")]
    Mismatch,
    #[error("the frozen Desktop manifest names an untrusted source")]
    Untrusted,
}

impl Manifest {
    /// Read a bounded manifest and verify every entry against the allowlisted policy.
    ///
    /// # Errors
    /// Fails closed on oversized, malformed, mismatched or untrusted content.
    pub fn read(
        path: &Path,
        apps: &[DesktopHarnessKind],
        model: &str,
    ) -> Result<(Self, String), ManifestError> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| {
                file.take(MAX_MANIFEST_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|_| ManifestError::Unreadable)?;
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::Invalid);
        }
        let manifest: Self = serde_json::from_slice(&bytes).map_err(|_| ManifestError::Invalid)?;
        manifest.verify(apps, Platform::current(), Architecture::current(), model)?;
        Ok((manifest, crate::report::digest(&bytes)))
    }

    /// # Errors
    /// Returns a closed error for any divergence from the selected cell or policy.
    pub fn verify(
        &self,
        apps: &[DesktopHarnessKind],
        platform: Platform,
        architecture: Architecture,
        model: &str,
    ) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION || self.suite != "desktop" || !valid_model(model) {
            return Err(ManifestError::Invalid);
        }
        let mut selected = apps.to_vec();
        selected.sort_unstable();
        selected.dedup();
        if self.platform != platform
            || self.architecture != architecture
            || self.model != model
            || self.apps.iter().map(Entry::app).collect::<Vec<_>>() != selected
        {
            return Err(ManifestError::Mismatch);
        }
        for entry in &self.apps {
            verify_entry(entry, policy(entry.app(), platform, architecture))?;
        }
        Ok(())
    }

    #[must_use]
    pub fn entry(&self, app: DesktopHarnessKind) -> Option<&Entry> {
        self.apps.iter().find(|entry| entry.app() == app)
    }
}

fn verify_entry(entry: &Entry, policy: Policy) -> Result<(), ManifestError> {
    let release = match entry {
        Entry::Blocked(blocker) => {
            let expected = match policy {
                Policy::Blocked { reason, .. } => reason,
                _ => BlockReason::ResolutionFailed,
            };
            return if blocker.reason == expected && blocker.evidence == policy.evidence() {
                Ok(())
            } else {
                Err(ManifestError::Untrusted)
            };
        }
        Entry::Frozen(release) => release,
    };
    let version = exact_version(&release.version).ok_or(ManifestError::Invalid)?;
    if release.runtime_version.as_deref().is_some_and(|runtime| {
        Version::parse(runtime).map_or(true, |version| version.to_string() != runtime)
    }) || release.channel != policy.channel()
    {
        return Err(ManifestError::Untrusted);
    }
    let digest = release.digest.as_deref();
    let (url_ok, format, installer, staged, source) = match policy {
        Policy::Apt {
            base,
            package,
            architecture,
        } => (
            release.url == apt_url(base, package, &version, architecture),
            PackageFormat::Deb,
            Installer::Checker,
            false,
            false,
        ),
        Policy::Sparkle { archive_prefix, .. } => (
            release.url == format!("{archive_prefix}{version}.zip"),
            PackageFormat::Zip,
            Installer::Checker,
            true,
            false,
        ),
        Policy::SquirrelMac { archive_prefix, .. } => (
            squirrel_mac_url(archive_prefix, &version, &release.url),
            PackageFormat::Zip,
            Installer::Checker,
            true,
            false,
        ),
        Policy::GithubAsset {
            repository,
            asset,
            format,
            installer,
        } => (
            release.url
                == format!("https://github.com/{repository}/releases/download/v{version}/{asset}"),
            format,
            installer,
            false,
            false,
        ),
        Policy::GithubSource { repository, .. } => (
            release.url == format!("https://github.com/{repository}.git"),
            PackageFormat::Source,
            Installer::External,
            false,
            true,
        ),
        Policy::Moving {
            url,
            format,
            installer,
        } => (release.url == url, format, installer, true, false),
        Policy::Blocked { .. } => return Err(ManifestError::Untrusted),
    };
    let identity_ok = if source {
        digest.is_none()
            && release
                .revision
                .as_deref()
                .is_some_and(|revision| lower_hex(revision, 40))
    } else {
        digest.is_some_and(sha256_digest) && release.revision.is_none()
    };
    if !url_ok
        || release.format != format
        || release.installer != installer
        || release.staged != staged
        || !identity_ok
    {
        return Err(ManifestError::Untrusted);
    }
    Ok(())
}

pub(crate) fn apt_url(base: &str, package: &str, version: &Version, architecture: &str) -> String {
    let initial = &package[..1];
    format!("{base}pool/main/{initial}/{package}/{package}_{version}_{architecture}.deb")
}

pub(crate) fn squirrel_mac_url(prefix: &str, version: &Version, url: &str) -> bool {
    url.strip_prefix(&format!("{prefix}{version}/Claude-"))
        .and_then(|rest| rest.strip_suffix(".zip"))
        .is_some_and(|hash| lower_hex(hash, 40))
}

/// Private staged copy of an entry's frozen bytes, named only by its digest.
#[must_use]
pub fn staged_path(artifacts: &Path, release: &Release) -> Option<PathBuf> {
    let hex = release.digest.as_deref()?.strip_prefix("sha256:")?;
    release
        .staged
        .then(|| artifacts.join(format!("{}-{hex}", release.app)))
}

/// Central-directory entries of a ZIP archive that passed traversal and size checks.
pub(crate) fn zip_entries(path: &Path) -> Result<Vec<inspect::zip::ZipEntry>, ()> {
    let entries = inspect::zip::entries(path).map_err(|_| ())?;
    inspect::zip::validate(&entries).map_err(|_| ())?;
    Ok(entries)
}
