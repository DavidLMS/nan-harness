use super::CompatibilityError;
use super::desktop::apply_desktop_verifications;
use super::environment::{automatic_refresh_enabled, compatibility_manifest_url};
use super::evidence::{apply_verifications, select_release};
use super::network::fetch_manifest;
use super::state::{
    CompatibilityState, CompatibilityStateStore, cache_is_fresh, source_fingerprint, unix_seconds,
};
use super::validation::validate_manifest;
use crate::desktop_compatibility::DesktopCompatibilityEntry;
use nan_harness_core::CompatibilityManifest;
use semver::Version;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    Disabled,
    Cached,
    Updated,
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

pub(super) async fn refresh_store(
    url: &str,
    store: &CompatibilityStateStore,
    base: &CompatibilityManifest,
) -> Result<RefreshOutcome, CompatibilityError> {
    let mut state = readable_state(store)?;
    if cache_is_fresh(&state, url)
        && state
            .cached_manifest
            .as_ref()
            .is_some_and(|cached| validate_manifest(cached, base).is_ok())
    {
        return Ok(RefreshOutcome::Cached);
    }
    let manifest = fetch_manifest(url, base).await?;
    state.last_checked_unix_seconds = Some(unix_seconds()?);
    state.source_fingerprint = Some(source_fingerprint(url));
    state.cached_manifest = Some(manifest);
    store.save(&state)?;
    Ok(RefreshOutcome::Updated)
}

/// Loads the cache, recovering from a document this binary cannot read.
///
/// Unreadable content is replaced by one bounded download; a filesystem failure is still
/// reported, because it usually means the state cannot be written either.
fn readable_state(
    store: &CompatibilityStateStore,
) -> Result<CompatibilityState, CompatibilityError> {
    match store.load() {
        Ok(state) => Ok(state),
        Err(CompatibilityError::ParseState(_) | CompatibilityError::UnsupportedStateSchema(_)) => {
            Ok(CompatibilityState::default())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn apply_cached_verifications(manifest: &mut CompatibilityManifest) {
    let Some((cached, release)) = cached_release() else {
        return;
    };
    if validate_manifest(&cached, manifest).is_ok() {
        let _ = apply_verifications(manifest, &release);
    }
}

/// Overlays the cached feed onto one effective Desktop entry.
///
/// Desktop evidence is applied through the registry rather than in each launcher, so every
/// Desktop surface observes the same effective record.
pub(crate) fn apply_cached_desktop_verifications(entry: &mut DesktopCompatibilityEntry) {
    let Some((cached, release)) = cached_release() else {
        return;
    };
    let Ok(base) = crate::discovery::bundled_compatibility_manifest() else {
        return;
    };
    if validate_manifest(&cached, &base).is_ok() {
        apply_desktop_verifications(entry, &release);
        super::desktop_checks::apply_checks(entry, &release);
    }
}

/// Returns the cached feed and the record for the exact running release.
///
/// Cached evidence is bound to the feed it came from: a different configured source, or none at
/// all, leaves the embedded evidence in effect.
fn cached_release() -> Option<(super::VerificationManifest, super::VerificationRelease)> {
    let url = compatibility_manifest_url()?;
    let store = CompatibilityStateStore::from_environment().ok()?;
    let state: CompatibilityState = store.load().ok()?;
    if !state.matches_source(&url) {
        return None;
    }
    let cached = state.cached_manifest?;
    let version = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    let release = select_release(&cached, &version)?.clone();
    Some((cached, release))
}
