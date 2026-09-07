//! Desktop evidence for the unified schema-v3 compatibility feed.
//!
//! The producer mirrors the client contract in
//! `crates/nan-harness-runtime/src/compatibility/desktop.rs`: records are adopted whole, live
//! evidence is never traded for a contract-only claim, and only records published for this
//! release are measured against this checkout's registry.

use super::validation::repository_root;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, format_description::well_known::Rfc3339};

const DESKTOP_SOURCE_PATH: &str = "crates/nan-harness-runtime/resources/desktop-compatibility.json";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DesktopVerificationEntry {
    pub(super) id: String,
    pub(super) platform: String,
    pub(super) evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_compatible_app_version: Option<Version>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_compatible_runtime_version: Option<Version>,
    pub(super) compatible_at: String,
}

/// A single Desktop update file dropped into the canary update directory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DesktopVerificationUpdate {
    #[serde(default)]
    pub(super) nan_harness_version: Option<Version>,
    id: String,
    platform: String,
    evidence: String,
    #[serde(default)]
    last_compatible_app_version: Option<Version>,
    #[serde(default)]
    last_compatible_runtime_version: Option<Version>,
    compatible_at: String,
}

impl DesktopVerificationUpdate {
    pub(super) fn into_entry(self) -> DesktopVerificationEntry {
        DesktopVerificationEntry {
            id: self.id,
            platform: self.platform,
            evidence: self.evidence,
            last_compatible_app_version: self.last_compatible_app_version,
            last_compatible_runtime_version: self.last_compatible_runtime_version,
            compatible_at: self.compatible_at,
        }
    }
}

/// What the embedded registry certifies for one surface and platform.
#[derive(Clone, Debug)]
pub(super) struct DesktopRequirement {
    pub(super) available: bool,
    pub(super) minimum_app_version: Option<Version>,
    pub(super) minimum_runtime_version: Option<Version>,
    pub(super) last_compatible_app_version: Option<Version>,
    pub(super) last_compatible_runtime_version: Option<Version>,
    pub(super) evidence: String,
    pub(super) compatible_at: String,
}

pub(super) type DesktopRequirements = BTreeMap<(String, String), DesktopRequirement>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registry {
    schema_version: u8,
    surfaces: Vec<RegistryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryEntry {
    id: String,
    platform: String,
    transport: String,
    evidence: String,
    #[serde(default)]
    minimum_app_version: Option<Version>,
    #[serde(default)]
    last_compatible_app_version: Option<Version>,
    #[serde(default)]
    minimum_runtime_version: Option<Version>,
    #[serde(default)]
    last_compatible_runtime_version: Option<Version>,
    compatible_at: String,
}

/// Reads the embedded Desktop registry that bounds every published Desktop record.
pub(super) fn desktop_requirements() -> Result<DesktopRequirements, String> {
    let source_path = repository_root().join(DESKTOP_SOURCE_PATH);
    let source = fs::read(&source_path)
        .map_err(|error| format!("could not read '{}': {error}", source_path.display()))?;
    let registry: Registry = serde_json::from_slice(&source).map_err(|error| {
        format!(
            "could not parse desktop compatibility registry '{}': {error}",
            source_path.display()
        )
    })?;
    if registry.schema_version != 1 {
        return Err(format!(
            "desktop compatibility registry schema {} is not supported",
            registry.schema_version
        ));
    }
    let mut requirements = DesktopRequirements::new();
    for entry in registry.surfaces {
        if !matches!(
            entry.evidence.as_str(),
            "live-verified" | "contract-only" | "unavailable"
        ) {
            return Err(format!(
                "desktop compatibility registry entry {} on {} has unknown evidence '{}'",
                entry.id, entry.platform, entry.evidence
            ));
        }
        let _ = entry.transport;
        let key = (entry.id.clone(), entry.platform.clone());
        if requirements
            .insert(
                key,
                DesktopRequirement {
                    available: entry.evidence != "unavailable",
                    minimum_app_version: entry.minimum_app_version,
                    minimum_runtime_version: entry.minimum_runtime_version,
                    last_compatible_app_version: entry.last_compatible_app_version,
                    last_compatible_runtime_version: entry.last_compatible_runtime_version,
                    evidence: entry.evidence,
                    compatible_at: entry.compatible_at,
                },
            )
            .is_some()
        {
            return Err(format!(
                "desktop compatibility registry contains duplicate {} on {}",
                entry.id, entry.platform
            ));
        }
    }
    Ok(requirements)
}

/// Builds the Desktop half of a generated release record from the embedded registry.
pub(super) fn bundled_desktop_verifications(
    requirements: &DesktopRequirements,
) -> Vec<DesktopVerificationEntry> {
    requirements
        .iter()
        .filter(|(_, requirement)| requirement.available)
        .map(|((id, platform), requirement)| DesktopVerificationEntry {
            id: id.clone(),
            platform: platform.clone(),
            evidence: requirement.evidence.clone(),
            last_compatible_app_version: requirement.last_compatible_app_version.clone(),
            last_compatible_runtime_version: requirement.last_compatible_runtime_version.clone(),
            compatible_at: normalize_timestamp(&requirement.compatible_at),
        })
        .collect()
}

/// Validates the Desktop records of one release.
///
/// `embedded_release` is true only for the release this checkout publishes. Records of another
/// release keep the platforms and minimums of the binary that produced them.
pub(super) fn validate_desktop_verifications(
    entries: &[DesktopVerificationEntry],
    requirements: &DesktopRequirements,
    embedded_release: bool,
    source: &str,
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for entry in entries {
        validate_desktop_structure(entry, source)?;
        if embedded_release {
            validate_against_registry(entry, requirements, source)?;
        }
        if !seen.insert((entry.id.clone(), entry.platform.clone())) {
            return Err(format!(
                "{source} contains duplicate Desktop entry for {} on {}",
                entry.id, entry.platform
            ));
        }
    }
    Ok(())
}

fn validate_desktop_structure(
    entry: &DesktopVerificationEntry,
    source: &str,
) -> Result<(), String> {
    if !matches!(entry.evidence.as_str(), "live-verified" | "contract-only") {
        return Err(format!(
            "{source} reports {} on {} with unpublishable evidence '{}'",
            entry.id, entry.platform, entry.evidence
        ));
    }
    parse_evidence_instant(&entry.compatible_at).ok_or_else(|| {
        format!(
            "{source} entry {} on {} has an invalid compatibleAt timestamp",
            entry.id, entry.platform
        )
    })?;
    // Live verification names the application version that was run.
    if entry.evidence == "live-verified" && entry.last_compatible_app_version.is_none() {
        return Err(format!(
            "{source} claims live verification of {} on {} without application evidence",
            entry.id, entry.platform
        ));
    }
    Ok(())
}

fn validate_against_registry(
    entry: &DesktopVerificationEntry,
    requirements: &DesktopRequirements,
    source: &str,
) -> Result<(), String> {
    let Some(requirement) = requirements.get(&(entry.id.clone(), entry.platform.clone())) else {
        return Err(format!(
            "{source} reports unknown Desktop surface {} on {}",
            entry.id, entry.platform
        ));
    };
    if !requirement.available {
        return Err(format!(
            "{source} cannot certify {} on {}, which has no supported surface",
            entry.id, entry.platform
        ));
    }
    check_minimum(
        entry.last_compatible_app_version.as_ref(),
        requirement.minimum_app_version.as_ref(),
        "application",
        entry,
        source,
    )?;
    check_minimum(
        entry.last_compatible_runtime_version.as_ref(),
        requirement.minimum_runtime_version.as_ref(),
        "runtime",
        entry,
        source,
    )?;
    if entry.evidence == "live-verified" {
        if requirement.minimum_app_version.is_some() && entry.last_compatible_app_version.is_none()
        {
            return Err(format!(
                "{source} claims live verification of {} on {} without application evidence",
                entry.id, entry.platform
            ));
        }
        if requirement.minimum_runtime_version.is_some()
            && entry.last_compatible_runtime_version.is_none()
        {
            return Err(format!(
                "{source} claims live verification of {} on {} without runtime evidence",
                entry.id, entry.platform
            ));
        }
    }
    Ok(())
}

fn check_minimum(
    version: Option<&Version>,
    minimum: Option<&Version>,
    track: &str,
    entry: &DesktopVerificationEntry,
    source: &str,
) -> Result<(), String> {
    if let (Some(version), Some(minimum)) = (version, minimum)
        && version < minimum
    {
        return Err(format!(
            "{source} reports {} on {} {track} version {version}, below minimum {minimum}",
            entry.id, entry.platform
        ));
    }
    Ok(())
}

/// Merges one Desktop update into a release. A record is adopted whole or left alone.
pub(super) fn merge_desktop_entry(
    entries: &mut Vec<DesktopVerificationEntry>,
    update: DesktopVerificationEntry,
    source: &str,
) -> Result<(), String> {
    let Some(current) = entries
        .iter_mut()
        .find(|entry| entry.id == update.id && entry.platform == update.platform)
    else {
        entries.push(update);
        return Ok(());
    };
    let update_instant = parse_evidence_instant(&update.compatible_at).ok_or_else(|| {
        format!(
            "{source} entry {} on {} has an invalid compatibleAt timestamp",
            update.id, update.platform
        )
    })?;
    let current_instant = parse_evidence_instant(&current.compatible_at).ok_or_else(|| {
        format!(
            "{source} established entry {} on {} has an invalid compatibleAt timestamp",
            current.id, current.platform
        )
    })?;
    if !adopts(current, &update, current_instant, update_instant) {
        return Ok(());
    }
    *current = update;
    Ok(())
}

fn adopts(
    current: &DesktopVerificationEntry,
    update: &DesktopVerificationEntry,
    current_instant: OffsetDateTime,
    update_instant: OffsetDateTime,
) -> bool {
    if update_instant < current_instant {
        return false;
    }
    match (current.evidence.as_str(), update.evidence.as_str()) {
        // Live verification is never traded for a contract-only claim, whatever its date.
        ("live-verified", "contract-only") => false,
        // A promotion replaces placeholder bounds with the pair that was actually verified.
        ("contract-only", "live-verified") => true,
        _ => {
            pair_does_not_regress(current, update)
                && (pair_advances(current, update) || update_instant > current_instant)
        }
    }
}

fn pair_does_not_regress(
    current: &DesktopVerificationEntry,
    update: &DesktopVerificationEntry,
) -> bool {
    bound_does_not_regress(
        current.last_compatible_app_version.as_ref(),
        update.last_compatible_app_version.as_ref(),
    ) && bound_does_not_regress(
        current.last_compatible_runtime_version.as_ref(),
        update.last_compatible_runtime_version.as_ref(),
    )
}

fn pair_advances(current: &DesktopVerificationEntry, update: &DesktopVerificationEntry) -> bool {
    bound_advances(
        current.last_compatible_app_version.as_ref(),
        update.last_compatible_app_version.as_ref(),
    ) || bound_advances(
        current.last_compatible_runtime_version.as_ref(),
        update.last_compatible_runtime_version.as_ref(),
    )
}

fn bound_does_not_regress(current: Option<&Version>, update: Option<&Version>) -> bool {
    match (current, update) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(current), Some(update)) => update >= current,
    }
}

fn bound_advances(current: Option<&Version>, update: Option<&Version>) -> bool {
    match (current, update) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(current), Some(update)) => update > current,
    }
}

/// The embedded registry records plain dates; the feed publishes RFC 3339 timestamps.
fn normalize_timestamp(value: &str) -> String {
    if OffsetDateTime::parse(value, &Rfc3339).is_ok() {
        return value.to_owned();
    }
    format!("{value}T00:00:00Z")
}

fn parse_evidence_instant(value: &str) -> Option<OffsetDateTime> {
    if let Ok(instant) = OffsetDateTime::parse(value, &Rfc3339) {
        return Some(instant);
    }
    let (year, remainder) = value.split_once('-')?;
    let (month, day) = remainder.split_once('-')?;
    let date = Date::from_calendar_date(
        year.parse().ok()?,
        Month::try_from(month.parse::<u8>().ok()?).ok()?,
        day.parse().ok()?,
    )
    .ok()?;
    Some(OffsetDateTime::new_in_offset(
        date,
        Time::MIDNIGHT,
        UtcOffset::UTC,
    ))
}
