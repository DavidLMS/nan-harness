use nan_harness_core::{CompatibilityManifest, HarnessKind};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationManifest {
    pub(super) schema_version: u8,
    pub(super) releases: Vec<VerificationRelease>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationEntry {
    pub(super) id: String,
    #[serde(default)]
    pub(super) last_compatible_version: Option<Version>,
    #[serde(default)]
    pub(super) compatible_at: Option<String>,
    #[serde(default)]
    pub(super) last_live_verified_version: Option<Version>,
    #[serde(default)]
    pub(super) live_verified_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationRelease {
    pub(super) nan_harness_version: Version,
    #[serde(alias = "harnesses")]
    pub(super) verifications: Vec<VerificationEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerificationUpdate {
    #[serde(default)]
    pub(super) nan_harness_version: Option<Version>,
    pub(super) id: String,
    #[serde(default)]
    last_compatible_version: Option<Version>,
    #[serde(default)]
    compatible_at: Option<String>,
    #[serde(default)]
    last_live_verified_version: Option<Version>,
    #[serde(default)]
    live_verified_at: Option<String>,
}

#[derive(Clone)]
pub(super) struct HarnessRequirement {
    pub(super) minimum_version: Version,
    pub(super) compatible_version: Version,
}

impl VerificationUpdate {
    pub(super) fn into_entry(self) -> VerificationEntry {
        VerificationEntry {
            id: self.id,
            last_compatible_version: self.last_compatible_version,
            compatible_at: self.compatible_at,
            last_live_verified_version: self.last_live_verified_version,
            live_verified_at: self.live_verified_at,
        }
    }
}

pub(super) fn validate_embedded_manifest(manifest: &CompatibilityManifest) -> Result<(), String> {
    if manifest.schema_version != CompatibilityManifest::SCHEMA_VERSION {
        return Err(format!(
            "embedded compatibility schema {} is not supported",
            manifest.schema_version
        ));
    }
    parse_timestamp(&manifest.tested_at, "testedAt")?;
    let mut ids = BTreeSet::new();
    for entry in &manifest.harnesses {
        if !ids.insert(entry.id) {
            return Err(format!(
                "embedded compatibility contains duplicate {}",
                entry.id
            ));
        }
        if entry.last_compatible_version < entry.minimum_version {
            return Err(format!(
                "embedded compatibility reports {} version {}, below minimum {}",
                entry.id, entry.last_compatible_version, entry.minimum_version
            ));
        }
        parse_timestamp(&entry.compatible_at, "compatibleAt")?;
        match (&entry.last_live_verified_version, &entry.live_verified_at) {
            (None, None) => {}
            (Some(version), Some(timestamp)) => {
                if version < &entry.minimum_version {
                    return Err(format!(
                        "embedded compatibility reports {} live version {}, below minimum {}",
                        entry.id, version, entry.minimum_version
                    ));
                }
                if version > &entry.last_compatible_version {
                    return Err(format!(
                        "embedded compatibility reports {} live version {} newer than compatible version {}",
                        entry.id, version, entry.last_compatible_version
                    ));
                }
                parse_timestamp(timestamp, "liveVerifiedAt")?;
            }
            _ => {
                return Err(format!(
                    "embedded compatibility entry {} has an incomplete live evidence pair",
                    entry.id
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn bundled_verification_release(source: &CompatibilityManifest) -> VerificationRelease {
    VerificationRelease {
        nan_harness_version: current_release_version(),
        verifications: source
            .harnesses
            .iter()
            .map(|entry| VerificationEntry {
                id: entry.id.to_string(),
                last_compatible_version: Some(entry.last_compatible_version.clone()),
                compatible_at: Some(entry.compatible_at.clone()),
                last_live_verified_version: entry.last_live_verified_version.clone(),
                live_verified_at: entry.live_verified_at.clone(),
            })
            .collect(),
    }
}

pub(super) fn validate_manifest_header(
    manifest: &VerificationManifest,
    schema_version: u8,
) -> Result<(), String> {
    if manifest.schema_version != schema_version {
        return Err(format!(
            "compatibility feed schema {} is not supported",
            manifest.schema_version
        ));
    }
    Ok(())
}

pub(super) fn validate_releases(
    releases: &[VerificationRelease],
    requirements: &BTreeMap<HarnessKind, HarnessRequirement>,
    source: &str,
) -> Result<(), String> {
    let mut release_versions = BTreeSet::new();
    for release in releases {
        if !release_versions.insert(release.nan_harness_version.clone()) {
            return Err(format!(
                "{source} contains duplicate release {}",
                release.nan_harness_version
            ));
        }
        let mut ids = BTreeSet::new();
        for entry in &release.verifications {
            let id = validate_verification_entry(entry, requirements, None, source)?;
            if let Some(id) = id
                && !ids.insert(id)
            {
                return Err(format!(
                    "{source} contains duplicate entry for {} in release {}",
                    id, release.nan_harness_version
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn apply_release_update(
    releases: &mut Vec<VerificationRelease>,
    update: VerificationRelease,
    requirements: &BTreeMap<HarnessKind, HarnessRequirement>,
    source: &str,
) -> Result<(), String> {
    let release = if let Some(release) = releases
        .iter_mut()
        .find(|release| release.nan_harness_version == update.nan_harness_version)
    {
        release
    } else {
        releases.push(VerificationRelease {
            nan_harness_version: update.nan_harness_version.clone(),
            verifications: Vec::new(),
        });
        releases.last_mut().expect("the release was just appended")
    };
    let mut ids = BTreeSet::new();
    for entry in &update.verifications {
        let existing_compatible = entry.id.parse::<HarnessKind>().ok().and_then(|id| {
            release
                .verifications
                .iter()
                .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id))
                .and_then(|existing| existing.last_compatible_version.as_ref())
        });
        let id = validate_verification_entry(entry, requirements, existing_compatible, source)?;
        if let Some(id) = id
            && !ids.insert(id)
        {
            return Err(format!("{source} contains duplicate entry for {id}"));
        }
    }
    for entry in update.verifications {
        let Ok(id) = entry.id.parse::<HarnessKind>() else {
            continue;
        };
        let requirement = requirements
            .get(&id)
            .expect("all known harnesses have embedded requirements");
        let existing = release
            .verifications
            .iter()
            .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id));
        if let Some(live_version) = &entry.last_live_verified_version {
            let compatible_version = entry
                .last_compatible_version
                .as_ref()
                .or_else(|| existing.and_then(|entry| entry.last_compatible_version.as_ref()))
                .unwrap_or(&requirement.compatible_version);
            if live_version > compatible_version {
                return Err(format!(
                    "{source} reports {id} live version {live_version} newer than compatible version {compatible_version}"
                ));
            }
        }
        let Some(existing) = release
            .verifications
            .iter_mut()
            .find(|existing| existing.id.parse::<HarnessKind>().ok() == Some(id))
        else {
            release.verifications.push(entry);
            continue;
        };
        merge_verification_entry(existing, &entry, source)?;
    }
    Ok(())
}

fn validate_verification_entry(
    entry: &VerificationEntry,
    requirements: &BTreeMap<HarnessKind, HarnessRequirement>,
    compatible_fallback: Option<&Version>,
    source: &str,
) -> Result<Option<HarnessKind>, String> {
    let compatible_at = validate_evidence_pair(
        entry,
        "compatible",
        entry.last_compatible_version.as_ref(),
        entry.compatible_at.as_ref(),
        source,
    )?;
    let live_at = validate_evidence_pair(
        entry,
        "live",
        entry.last_live_verified_version.as_ref(),
        entry.live_verified_at.as_ref(),
        source,
    )?;
    if compatible_at.is_none() && live_at.is_none() {
        return Err(format!("{source} entry {} contains no evidence", entry.id));
    }
    let Ok(id) = entry.id.parse::<HarnessKind>() else {
        return Ok(None);
    };
    let Some(requirement) = requirements.get(&id) else {
        return Ok(None);
    };
    if let Some(version) = &entry.last_compatible_version
        && version < &requirement.minimum_version
    {
        return Err(format!(
            "{source} reports {id} version {version}, below minimum {}",
            requirement.minimum_version
        ));
    }
    if let Some(version) = &entry.last_live_verified_version
        && version < &requirement.minimum_version
    {
        return Err(format!(
            "{source} reports {id} live version {version}, below minimum {}",
            requirement.minimum_version
        ));
    }
    if let Some(live_version) = &entry.last_live_verified_version {
        let compatible_version = entry
            .last_compatible_version
            .as_ref()
            .or(compatible_fallback)
            .unwrap_or(&requirement.compatible_version);
        if live_version > compatible_version {
            return Err(format!(
                "{source} reports {id} live version {live_version} newer than compatible version {compatible_version}"
            ));
        }
    }
    Ok(Some(id))
}

fn validate_evidence_pair(
    entry: &VerificationEntry,
    track: &'static str,
    version: Option<&Version>,
    timestamp: Option<&String>,
    source: &str,
) -> Result<Option<OffsetDateTime>, String> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(_), Some(timestamp)) => {
            parse_timestamp(timestamp, &format!("{source} {} {track}", entry.id)).map(Some)
        }
        _ => Err(format!(
            "{source} entry {} has an incomplete {track} evidence pair",
            entry.id
        )),
    }
}

pub(super) fn merge_verification_entry(
    current: &mut VerificationEntry,
    update: &VerificationEntry,
    source: &str,
) -> Result<(), String> {
    merge_evidence_pair(
        &mut current.last_compatible_version,
        &mut current.compatible_at,
        update.last_compatible_version.as_ref(),
        update.compatible_at.as_ref(),
        &current.id,
        "compatible",
        source,
    )?;
    merge_evidence_pair(
        &mut current.last_live_verified_version,
        &mut current.live_verified_at,
        update.last_live_verified_version.as_ref(),
        update.live_verified_at.as_ref(),
        &current.id,
        "live",
        source,
    )
}

pub(super) fn merge_evidence_pair(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    update_version: Option<&Version>,
    update_at: Option<&String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<(), String> {
    let Some((update_version, update_at, update_instant)) =
        validate_update_evidence(update_version, update_at, id, track, source)?
    else {
        return Ok(());
    };
    let Some((current_version_value, current_at_value)) = current_evidence_pair(
        current_version.as_ref(),
        current_at.as_ref(),
        id,
        track,
        source,
    )?
    else {
        *current_version = Some(update_version.clone());
        *current_at = Some(update_at.clone());
        return Ok(());
    };
    merge_existing_evidence(
        current_version,
        current_at,
        &current_version_value,
        &current_at_value,
        update_version,
        update_at,
        update_instant,
        id,
        track,
        source,
    )
}

fn validate_update_evidence<'a>(
    version: Option<&'a Version>,
    timestamp: Option<&'a String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<Option<(&'a Version, &'a String, OffsetDateTime)>, String> {
    let Some(version) = version else {
        return Ok(None);
    };
    let Some(timestamp) = timestamp else {
        return Err(format!(
            "{source} entry {id} has an incomplete {track} evidence pair"
        ));
    };
    let instant = parse_timestamp(timestamp, &format!("{source} {id} {track}"))?;
    Ok(Some((version, timestamp, instant)))
}

fn current_evidence_pair(
    version: Option<&Version>,
    timestamp: Option<&String>,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<Option<(Version, String)>, String> {
    match (version, timestamp) {
        (None, None) => Ok(None),
        (Some(version), Some(timestamp)) => Ok(Some((version.clone(), timestamp.clone()))),
        _ => Err(format!(
            "{source} entry {id} has an incomplete {track} evidence pair"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_existing_evidence(
    current_version: &mut Option<Version>,
    current_at: &mut Option<String>,
    current_version_value: &Version,
    current_at_value: &str,
    update_version: &Version,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<(), String> {
    match update_version.cmp(current_version_value) {
        std::cmp::Ordering::Greater => {
            *current_version = Some(update_version.clone());
            *current_at = Some(newer_timestamp(
                current_at_value,
                update_at,
                update_instant,
                id,
                track,
                source,
            )?);
        }
        std::cmp::Ordering::Equal => {
            if timestamp_is_newer(current_at_value, update_instant, id, track, source)? {
                *current_at = Some(update_at.to_owned());
            }
        }
        std::cmp::Ordering::Less => {}
    }
    Ok(())
}

fn newer_timestamp(
    current_at: &str,
    update_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<String, String> {
    if timestamp_is_newer(current_at, update_instant, id, track, source)? {
        Ok(update_at.to_owned())
    } else {
        Ok(current_at.to_owned())
    }
}

fn timestamp_is_newer(
    current_at: &str,
    update_instant: OffsetDateTime,
    id: &str,
    track: &'static str,
    source: &str,
) -> Result<bool, String> {
    let current_instant = parse_timestamp(current_at, &format!("{source} {id} {track}"))?;
    Ok(update_instant > current_instant)
}

fn parse_timestamp(value: &str, field: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| format!("{field} must be a valid RFC3339 timestamp"))
}

pub(super) fn current_release_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace package version should be valid")
}
