//! Read official latest metadata once. Every network source is injectable for tests.

use super::{
    BlockReason, Blocker, Entry, Installer, Manifest, PackageFormat, Policy, Release,
    SCHEMA_VERSION, apt_url, exact_version, inspect, policy, squirrel_mac_url, staged_path,
    valid_model,
};
use crate::catalog::diagnostic;
use crate::install::FetchFailure;
use crate::report::{Architecture, Platform};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::Path;

const METADATA_BYTES: u64 = 4 * 1024 * 1024;

/// Official network access used by resolution. Implementations never add credentials.
pub trait Fetch {
    /// Return a bounded official metadata body.
    fn metadata(
        &mut self,
        url: &str,
        limit: u64,
    ) -> impl Future<Output = Result<Vec<u8>, FetchFailure>>;
    /// Save official artifact bytes to a new private file.
    fn artifact(
        &mut self,
        url: &str,
        destination: &Path,
    ) -> impl Future<Output = Result<(), FetchFailure>>;
    /// Read the version from staged bytes; tests replace only platform-tool formats.
    ///
    /// # Errors
    /// Rejects ambiguous metadata or uncertain read-only image cleanup.
    fn inspect(
        &mut self,
        app: DesktopHarnessKind,
        format: PackageFormat,
        path: &Path,
    ) -> Result<Version, inspect::InspectError> {
        inspect::artifact_version(app, format, path)
    }
}

/// HTTPS-only official fetches without authentication headers.
pub struct OfficialFetch;

impl Fetch for OfficialFetch {
    async fn metadata(&mut self, url: &str, limit: u64) -> Result<Vec<u8>, FetchFailure> {
        crate::install::fetch_bounded_detailed(url, limit).await
    }

    async fn artifact(&mut self, url: &str, destination: &Path) -> Result<(), FetchFailure> {
        crate::install::download_file_detailed(url, destination, crate::install::MAX_DOWNLOAD_BYTES)
            .await
    }
}

/// Resolution could not leave the staging directory in a known state.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    #[error("the model identifier is invalid")]
    Model,
    #[error("an installer image may still be mounted; later apps were not resolved")]
    CleanupUncertain,
}

enum Failure {
    /// A closed, app-local failure; independent apps continue.
    Unresolved(ResolveFailure),
    /// Mount or cleanup state is unknown; successors must not run.
    CleanupUncertain,
}

#[derive(Debug)]
enum ResolveFailure {
    Transport {
        failure: FetchFailure,
        operation: diagnostic::TransportOperation,
    },
    MetadataParse,
    ArtifactSelection,
    ArtifactVersion,
    ArtifactStaging,
}

fn emit_resolution_diagnostic(app: DesktopHarnessKind, failure: ResolveFailure) {
    let (error_category, transport_category, operation, http_status) = match failure {
        ResolveFailure::Transport { failure, operation } => (
            diagnostic::ErrorCategory::ResolutionTransport,
            Some(transport_category(&failure)),
            operation,
            match failure {
                FetchFailure::HttpStatus(status) => Some(status),
                _ => None,
            },
        ),
        other => (
            resolution_category(&other),
            None,
            diagnostic::TransportOperation::Metadata,
            None,
        ),
    };
    if let Some(transport_category) = transport_category {
        diagnostic::emit_transport(diagnostic::TransportEvent {
            schema_version: 1,
            app,
            stage: diagnostic::Stage::FrozenResolution,
            error_category,
            reason: crate::report::Reason::VersionUnknown,
            transport_category,
            operation,
            http_status,
        });
        return;
    }
    diagnostic::emit(diagnostic::Event {
        schema_version: 1,
        app,
        stage: diagnostic::Stage::FrozenResolution,
        error_category,
        reason: crate::report::Reason::VersionUnknown,
        os_error: None,
    });
}

fn resolution_category(failure: &ResolveFailure) -> diagnostic::ErrorCategory {
    match failure {
        ResolveFailure::Transport { .. } => diagnostic::ErrorCategory::ResolutionTransport,
        ResolveFailure::MetadataParse => diagnostic::ErrorCategory::MetadataParse,
        ResolveFailure::ArtifactSelection => diagnostic::ErrorCategory::ArtifactSelection,
        ResolveFailure::ArtifactVersion => diagnostic::ErrorCategory::ArtifactVersion,
        ResolveFailure::ArtifactStaging => diagnostic::ErrorCategory::ArtifactStaging,
    }
}

fn transport_category(failure: &FetchFailure) -> diagnostic::TransportCategory {
    match failure {
        FetchFailure::InvalidUrl => diagnostic::TransportCategory::InvalidUrl,
        FetchFailure::Policy => diagnostic::TransportCategory::Policy,
        FetchFailure::ClientSetup => diagnostic::TransportCategory::ClientSetup,
        FetchFailure::Timeout => diagnostic::TransportCategory::Timeout,
        FetchFailure::Connect => diagnostic::TransportCategory::Connect,
        FetchFailure::HttpStatus(_) => diagnostic::TransportCategory::HttpStatus,
        FetchFailure::Request => diagnostic::TransportCategory::Request,
        FetchFailure::BodyRead => diagnostic::TransportCategory::BodyRead,
        FetchFailure::BodyBound => diagnostic::TransportCategory::BodyBound,
        FetchFailure::LocalIo(_) => diagnostic::TransportCategory::LocalIo,
    }
}

fn resolution_transport(
    failure: FetchFailure,
    operation: diagnostic::TransportOperation,
) -> Failure {
    match failure {
        FetchFailure::LocalIo(_) => Failure::Unresolved(ResolveFailure::ArtifactStaging),
        failure => Failure::Unresolved(ResolveFailure::Transport { failure, operation }),
    }
}

async fn metadata(fetch: &mut impl Fetch, url: &str) -> Result<Vec<u8>, Failure> {
    fetch
        .metadata(url, METADATA_BYTES)
        .await
        .map_err(|failure| resolution_transport(failure, diagnostic::TransportOperation::Metadata))
}

/// Resolve every selected app exactly once for one native target.
///
/// # Errors
/// Only an invalid model or cleanup uncertainty stops resolution; app-local
/// failures become closed `resolution-failed` blockers without versions.
pub async fn resolve(
    apps: &[DesktopHarnessKind],
    platform: Platform,
    architecture: Architecture,
    model: &str,
    artifacts: &Path,
    fetch: &mut impl Fetch,
) -> Result<Manifest, ResolveError> {
    if !valid_model(model) {
        return Err(ResolveError::Model);
    }
    let mut selected = apps.to_vec();
    selected.sort_unstable();
    selected.dedup();
    let mut entries = Vec::new();
    for app in selected {
        entries.push(resolve_entry(app, platform, architecture, artifacts, fetch).await?);
    }
    Ok(Manifest {
        schema_version: SCHEMA_VERSION,
        suite: "desktop".into(),
        platform,
        architecture,
        model: model.into(),
        apps: entries,
    })
}

/// Resolve one app; app-local failures become a closed blocker without a version.
///
/// # Errors
/// Only cleanup uncertainty is returned, so callers stop before later apps.
pub async fn resolve_entry(
    app: DesktopHarnessKind,
    platform: Platform,
    architecture: Architecture,
    artifacts: &Path,
    fetch: &mut impl Fetch,
) -> Result<Entry, ResolveError> {
    let policy = policy(app, platform, architecture);
    match resolve_app(app, policy, artifacts, fetch).await {
        Ok(entry) => Ok(entry),
        Err(Failure::CleanupUncertain) => Err(ResolveError::CleanupUncertain),
        Err(Failure::Unresolved(failure)) => {
            emit_resolution_diagnostic(app, failure);
            eprintln!("{app}: official latest version could not be frozen");
            Ok(Entry::Blocked(Blocker {
                app,
                reason: BlockReason::ResolutionFailed,
                evidence: policy.evidence(),
            }))
        }
    }
}

async fn resolve_app(
    app: DesktopHarnessKind,
    policy: Policy,
    artifacts: &Path,
    fetch: &mut impl Fetch,
) -> Result<Entry, Failure> {
    let entry = match policy {
        Policy::Blocked { reason, evidence } => Entry::Blocked(Blocker {
            app,
            reason,
            evidence: evidence.into(),
        }),
        Policy::Apt {
            base,
            package,
            architecture,
        } => {
            let index = metadata(
                fetch,
                &format!("{base}dists/stable/main/binary-{architecture}/Packages"),
            )
            .await?;
            let text = std::str::from_utf8(&index)
                .map_err(|_| Failure::Unresolved(ResolveFailure::MetadataParse))?;
            let (version, sha256) = newest_apt(text, package, architecture)
                .ok_or(Failure::Unresolved(ResolveFailure::ArtifactSelection))?;
            let mut frozen = release(
                app,
                policy.channel(),
                &version,
                apt_url(base, package, &version, architecture),
                PackageFormat::Deb,
                Installer::Checker,
                false,
            );
            frozen.digest = Some(format!("sha256:{sha256}"));
            Entry::Frozen(frozen)
        }
        Policy::Sparkle {
            appcast,
            archive_prefix,
            hardware,
        } => {
            let body = metadata(fetch, appcast).await?;
            let text = std::str::from_utf8(&body)
                .map_err(|_| Failure::Unresolved(ResolveFailure::MetadataParse))?;
            let version = newest_sparkle(text, archive_prefix, hardware)
                .ok_or(Failure::Unresolved(ResolveFailure::ArtifactSelection))?;
            let frozen = release(
                app,
                policy.channel(),
                &version,
                format!("{archive_prefix}{version}.zip"),
                PackageFormat::Zip,
                Installer::Checker,
                true,
            );
            Entry::Frozen(stage(frozen, artifacts, fetch, false).await?)
        }
        Policy::SquirrelMac {
            releases,
            archive_prefix,
        } => {
            let body = metadata(fetch, releases).await?;
            let (version, url) = squirrel_mac(&body, archive_prefix)
                .ok_or(Failure::Unresolved(ResolveFailure::ArtifactSelection))?;
            let frozen = release(
                app,
                policy.channel(),
                &version,
                url,
                PackageFormat::Zip,
                Installer::Checker,
                true,
            );
            Entry::Frozen(stage(frozen, artifacts, fetch, false).await?)
        }
        Policy::GithubAsset { .. } | Policy::GithubSource { .. } => {
            Entry::Frozen(github_release(app, policy, fetch).await?)
        }
        Policy::Moving {
            url,
            format,
            installer,
        } => {
            moving_entry(
                app,
                policy.channel(),
                url,
                format,
                installer,
                artifacts,
                fetch,
            )
            .await?
        }
    };
    Ok(entry)
}

fn release(
    app: DesktopHarnessKind,
    channel: String,
    version: &Version,
    url: String,
    format: PackageFormat,
    installer: Installer,
    staged: bool,
) -> Release {
    Release {
        app,
        version: version.to_string(),
        runtime_version: None,
        channel,
        url,
        format,
        digest: None,
        revision: None,
        staged,
        installer,
    }
}

async fn moving_entry(
    app: DesktopHarnessKind,
    channel: String,
    url: &'static str,
    format: PackageFormat,
    installer: Installer,
    artifacts: &Path,
    fetch: &mut impl Fetch,
) -> Result<Entry, Failure> {
    let release = Release {
        app,
        version: Version::new(0, 0, 0).to_string(),
        runtime_version: None,
        channel,
        url: url.into(),
        format,
        digest: None,
        revision: None,
        staged: true,
        installer,
    };
    Ok(Entry::Frozen(stage(release, artifacts, fetch, true).await?))
}

/// GitHub releases bind either a downloadable asset digest or an exact source
/// revision, never a moving branch or a product version inferred from a date tag.
async fn github_release(
    app: DesktopHarnessKind,
    policy: Policy,
    fetch: &mut impl Fetch,
) -> Result<Release, Failure> {
    let (version, url, format, installer, digest, revision) = match policy {
        Policy::GithubAsset {
            repository,
            asset,
            format,
            installer,
        } => {
            let body = fetch
                .metadata(
                    &format!("https://api.github.com/repos/{repository}/releases/latest"),
                    METADATA_BYTES,
                )
                .await
                .map_err(|failure| {
                    resolution_transport(failure, diagnostic::TransportOperation::Metadata)
                })?;
            let (version, sha256) = github_asset(&body, repository, asset)
                .ok_or(Failure::Unresolved(ResolveFailure::ArtifactSelection))?;
            let url =
                format!("https://github.com/{repository}/releases/download/v{version}/{asset}");
            (
                version,
                url,
                format,
                installer,
                Some(format!("sha256:{sha256}")),
                None,
            )
        }
        Policy::GithubSource {
            repository,
            package_json,
        } => {
            let (version, revision) = github_source(repository, package_json, fetch)
                .await
                .ok_or(Failure::Unresolved(ResolveFailure::ArtifactSelection))?;
            (
                version,
                format!("https://github.com/{repository}.git"),
                PackageFormat::Source,
                Installer::External,
                None,
                Some(revision),
            )
        }
        _ => return Err(Failure::Unresolved(ResolveFailure::ArtifactSelection)),
    };
    Ok(Release {
        app,
        version: version.to_string(),
        runtime_version: None,
        channel: policy.channel(),
        url,
        format,
        digest,
        revision,
        staged: false,
        installer,
    })
}

/// Download once, bind the local SHA-256, and optionally read the version from those bytes.
/// Without `measure`, the version already read from official metadata is kept.
async fn stage(
    mut release: Release,
    artifacts: &Path,
    fetch: &mut impl Fetch,
    measure: bool,
) -> Result<Release, Failure> {
    let partial = artifacts.join(format!("{}.partial", release.app));
    let result = async {
        fetch
            .artifact(&release.url, &partial)
            .await
            .map_err(|failure| {
                resolution_transport(failure, diagnostic::TransportOperation::Artifact)
            })?;
        let digest = crate::install::sha256_file(&partial)
            .map_err(|_| Failure::Unresolved(ResolveFailure::ArtifactStaging))?;
        if measure {
            match fetch.inspect(release.app, release.format, &partial) {
                Ok(version) => release.version = version.to_string(),
                Err(inspect::InspectError::CleanupUncertain) => {
                    return Err(Failure::CleanupUncertain);
                }
                Err(inspect::InspectError::Unresolved) => {
                    return Err(Failure::Unresolved(ResolveFailure::ArtifactVersion));
                }
            }
        }
        release.digest = Some(format!("sha256:{digest}"));
        let staged = staged_path(artifacts, &release)
            .ok_or(Failure::Unresolved(ResolveFailure::ArtifactStaging))?;
        std::fs::rename(&partial, staged)
            .map_err(|_| Failure::Unresolved(ResolveFailure::ArtifactStaging))?;
        Ok(release)
    }
    .await;
    if matches!(result, Err(Failure::Unresolved(_))) {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

/// Newest exact version for one package and architecture, bound to its SHA-256.
pub(crate) fn newest_apt(
    text: &str,
    package: &str,
    architecture: &str,
) -> Option<(Version, String)> {
    let mut candidates = BTreeMap::new();
    for paragraph in text
        .split("\n\n")
        .filter(|paragraph| !paragraph.trim().is_empty())
    {
        let mut fields = BTreeMap::new();
        for line in paragraph.lines().filter(|line| !line.starts_with(' ')) {
            let (name, value) = line.split_once(": ")?;
            if fields.insert(name, value).is_some() {
                return None;
            }
        }
        if fields.get("Package") != Some(&package)
            || fields.get("Architecture") != Some(&architecture)
        {
            continue;
        }
        let version = exact_version(fields.get("Version")?)?;
        let sha256 = fields.get("SHA256")?.to_ascii_lowercase();
        let filename = apt_url("", package, &version, architecture);
        if fields.get("Filename") != Some(&filename.as_str())
            || sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || candidates.insert(version, sha256).is_some()
        {
            return None;
        }
    }
    candidates.pop_last()
}

fn element<'a>(text: &'a str, name: &str) -> Result<Option<&'a str>, ()> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let Some(start) = text.find(&open) else {
        return Ok(None);
    };
    let rest = &text[start + open.len()..];
    let end = rest.find(&close).ok_or(())?;
    if rest[end + close.len()..].contains(&open) {
        return Err(());
    }
    Ok(Some(rest[..end].trim()))
}

/// Parse Sparkle items conservatively: one version, one full enclosure, no deltas.
pub(crate) fn newest_sparkle(text: &str, archive_prefix: &str, hardware: &str) -> Option<Version> {
    let mut versions = BTreeSet::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("<item>") {
        let after = &remaining[start + "<item>".len()..];
        let end = after.find("</item>")?;
        let mut item = after[..end].to_owned();
        remaining = &after[end + "</item>".len()..];
        if let Some(delta) = item.find("<sparkle:deltas>") {
            let close = item[delta..].find("</sparkle:deltas>")? + delta;
            item.replace_range(delta..close + "</sparkle:deltas>".len(), "");
        }
        if element(&item, "sparkle:hardwareRequirements")
            .ok()?
            .is_some_and(|value| value != hardware)
        {
            continue;
        }
        let version = exact_version(element(&item, "sparkle:shortVersionString").ok()??)?;
        let mut enclosures = item.match_indices("<enclosure ");
        let (position, _) = enclosures.next()?;
        if enclosures.next().is_some() {
            return None;
        }
        let url = item[position..].split_once("url=\"")?.1.split_once('"')?.0;
        if url != format!("{archive_prefix}{version}.zip") || !versions.insert(version) {
            return None;
        }
    }
    versions.pop_last()
}

pub(crate) fn squirrel_mac(body: &[u8], archive_prefix: &str) -> Option<(Version, String)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let current = value.get("currentRelease")?.as_str()?;
    let version = exact_version(current)?;
    let mut matching = value
        .get("releases")?
        .as_array()?
        .iter()
        .filter(|release| release.get("version").and_then(|v| v.as_str()) == Some(current));
    let selected = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    let update = selected.get("updateTo")?;
    let url = update.get("url")?.as_str()?;
    (update.get("version")?.as_str()? == current && squirrel_mac_url(archive_prefix, &version, url))
        .then(|| (version, url.to_owned()))
}

pub(crate) fn github_asset(
    body: &[u8],
    repository: &str,
    asset: &str,
) -> Option<(Version, String)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    if value.get("draft")?.as_bool()? || value.get("prerelease")?.as_bool()? {
        return None;
    }
    let version = exact_version(value.get("tag_name")?.as_str()?.strip_prefix('v')?)?;
    let expected = format!("https://github.com/{repository}/releases/download/v{version}/{asset}");
    let mut matches = value
        .get("assets")?
        .as_array()?
        .iter()
        .filter(|entry| entry.get("name").and_then(|name| name.as_str()) == Some(asset));
    let selected = matches.next()?;
    if matches.next().is_some() || selected.get("browser_download_url")?.as_str()? != expected {
        return None;
    }
    let digest = selected.get("digest")?.as_str()?;
    super::sha256_digest(digest).then(|| (version, digest["sha256:".len()..].to_owned()))
}

async fn github_source(
    repository: &str,
    package_json: &str,
    fetch: &mut impl Fetch,
) -> Option<(Version, String)> {
    let api = format!("https://api.github.com/repos/{repository}");
    let latest: serde_json::Value = serde_json::from_slice(
        &fetch
            .metadata(&format!("{api}/releases/latest"), METADATA_BYTES)
            .await
            .ok()?,
    )
    .ok()?;
    if latest.get("draft")?.as_bool()? || latest.get("prerelease")?.as_bool()? {
        return None;
    }
    let tag = latest.get("tag_name")?.as_str()?;
    exact_version(tag.strip_prefix('v')?)?;
    let mut object = git_object(
        &fetch
            .metadata(&format!("{api}/git/ref/tags/{tag}"), 65_536)
            .await
            .ok()?,
    )?;
    // Annotated tags are dereferenced once; nested tag chains are not followed.
    if object.0 == "tag" {
        object = git_object(
            &fetch
                .metadata(&format!("{api}/git/tags/{}", object.1), 65_536)
                .await
                .ok()?,
        )?;
    }
    if object.0 != "commit" {
        return None;
    }
    let package: serde_json::Value = serde_json::from_slice(
        &fetch
            .metadata(
                &format!(
                    "https://raw.githubusercontent.com/{repository}/{}/{package_json}",
                    object.1
                ),
                1024 * 1024,
            )
            .await
            .ok()?,
    )
    .ok()?;
    let version = exact_version(package.get("version")?.as_str()?)?;
    Some((version, object.1))
}

fn git_object(body: &[u8]) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let object = value.get("object")?;
    let kind = object.get("type")?.as_str()?;
    let sha = object.get("sha")?.as_str()?;
    super::lower_hex(sha, 40).then(|| (kind.to_owned(), sha.to_owned()))
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    struct FailingFetch;

    impl Fetch for FailingFetch {
        async fn metadata(&mut self, _: &str, _: u64) -> Result<Vec<u8>, FetchFailure> {
            Err(FetchFailure::HttpStatus(403))
        }

        async fn artifact(&mut self, _: &str, _: &Path) -> Result<(), FetchFailure> {
            Err(FetchFailure::Connect)
        }
    }

    #[test]
    fn resolver_failure_subtypes_are_closed_and_non_sensitive() {
        assert_eq!(
            resolution_category(&ResolveFailure::Transport {
                failure: FetchFailure::Connect,
                operation: diagnostic::TransportOperation::Artifact,
            }),
            diagnostic::ErrorCategory::ResolutionTransport
        );
        assert_eq!(
            resolution_category(&ResolveFailure::MetadataParse),
            diagnostic::ErrorCategory::MetadataParse
        );
        assert_eq!(
            resolution_category(&ResolveFailure::ArtifactSelection),
            diagnostic::ErrorCategory::ArtifactSelection
        );
        assert_eq!(
            resolution_category(&ResolveFailure::ArtifactVersion),
            diagnostic::ErrorCategory::ArtifactVersion
        );
        assert_eq!(
            resolution_category(&ResolveFailure::ArtifactStaging),
            diagnostic::ErrorCategory::ArtifactStaging
        );
        for (failure, expected) in [
            (
                FetchFailure::Timeout,
                diagnostic::TransportCategory::Timeout,
            ),
            (
                FetchFailure::Connect,
                diagnostic::TransportCategory::Connect,
            ),
            (
                FetchFailure::HttpStatus(403),
                diagnostic::TransportCategory::HttpStatus,
            ),
            (
                FetchFailure::BodyBound,
                diagnostic::TransportCategory::BodyBound,
            ),
        ] {
            assert_eq!(transport_category(&failure), expected);
        }
    }

    #[test]
    fn transport_diagnostics_allow_only_status_detail() {
        let status = diagnostic::TransportEvent {
            schema_version: 1,
            app: DesktopHarnessKind::Claude,
            stage: diagnostic::Stage::FrozenResolution,
            error_category: diagnostic::ErrorCategory::ResolutionTransport,
            reason: crate::report::Reason::VersionUnknown,
            transport_category: diagnostic::TransportCategory::HttpStatus,
            operation: diagnostic::TransportOperation::Artifact,
            http_status: Some(403),
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["transportCategory"], "http-status");
        assert_eq!(value["httpStatus"], 403);
        assert!(value.get("url").is_none());
        assert!(value.get("body").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("credentials").is_none());
        assert!(
            serde_json::from_value::<diagnostic::TransportEvent>(serde_json::json!({
                "schemaVersion": 1,
                "app": "claude-desktop",
                "stage": "frozen-resolution",
                "errorCategory": "resolution-transport",
                "reason": "version-unknown",
            "transportCategory": "http-status",
            "operation": "artifact",
                "httpStatus": 403,
                "body": "must-not-escape"
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn failed_claude_windows_resolution_emits_a_closed_blocker() {
        let artifacts = tempfile::tempdir().unwrap();
        let entry = resolve_entry(
            DesktopHarnessKind::Claude,
            Platform::Windows,
            Architecture::X86_64,
            artifacts.path(),
            &mut FailingFetch,
        )
        .await
        .unwrap();
        assert!(matches!(
            entry,
            Entry::Blocked(Blocker {
                reason: BlockReason::ResolutionFailed,
                ..
            })
        ));
    }
}
