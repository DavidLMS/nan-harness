use super::CompatibilityError;
use super::environment::{automatic_refresh_enabled, compatibility_manifest_url};
use super::evidence::{apply_verifications, select_release};
use super::network::fetch_manifest;
use super::state::{CompatibilityStateStore, cache_is_fresh, unix_seconds};
use super::validation::validate_manifest;
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
    let mut state = store.load()?;
    if cache_is_fresh(&state)
        && state
            .cached_manifest
            .as_ref()
            .is_some_and(|cached| validate_manifest(cached, base).is_ok())
    {
        return Ok(RefreshOutcome::Cached);
    }
    let manifest = fetch_manifest(url, base).await?;
    state.last_checked_unix_seconds = Some(unix_seconds()?);
    state.cached_manifest = Some(manifest);
    store.save(&state)?;
    Ok(RefreshOutcome::Updated)
}

pub(crate) fn apply_cached_verifications(manifest: &mut CompatibilityManifest) {
    let Ok(store) = CompatibilityStateStore::from_environment() else {
        return;
    };
    let Ok(state) = store.load() else {
        return;
    };
    if let Some(cached) = state.cached_manifest
        && validate_manifest(&cached, manifest).is_ok()
    {
        let Ok(version) = Version::parse(env!("CARGO_PKG_VERSION")) else {
            return;
        };
        if let Some(release) = select_release(&cached, &version) {
            let _ = apply_verifications(manifest, release);
        }
    }
}
