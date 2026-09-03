use super::DiscoveryError;
use nan_harness_core::CompatibilityManifest;
use std::collections::BTreeSet;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const COMPATIBILITY_MANIFEST: &str = include_str!("../../resources/compatibility.json");

/// Loads the compatibility manifest embedded in the runtime binary.
///
/// # Errors
///
/// Returns [`DiscoveryError`] if the bundled resource cannot be deserialized or violates its
/// compatibility contract.
pub fn bundled_compatibility_manifest() -> Result<CompatibilityManifest, DiscoveryError> {
    let manifest: CompatibilityManifest =
        serde_json::from_str(COMPATIBILITY_MANIFEST).map_err(DiscoveryError::InvalidManifest)?;
    validate_embedded_manifest(&manifest).map_err(DiscoveryError::InvalidManifestContract)?;
    Ok(manifest)
}

fn validate_embedded_manifest(manifest: &CompatibilityManifest) -> Result<(), String> {
    if manifest.schema_version != CompatibilityManifest::SCHEMA_VERSION {
        return Err(format!(
            "schema {} is not supported",
            manifest.schema_version
        ));
    }
    parse_timestamp(&manifest.tested_at, "testedAt")?;
    let mut ids = BTreeSet::new();
    for entry in &manifest.harnesses {
        if !ids.insert(entry.id) {
            return Err(format!("duplicate harness entry for {}", entry.id));
        }
        if entry.last_compatible_version < entry.minimum_version {
            return Err(format!(
                "{} compatible version {} is below minimum {}",
                entry.id, entry.last_compatible_version, entry.minimum_version
            ));
        }
        parse_timestamp(&entry.compatible_at, "compatibleAt")?;
        match (&entry.last_live_verified_version, &entry.live_verified_at) {
            (None, None) => {}
            (Some(version), Some(timestamp)) => {
                if version < &entry.minimum_version {
                    return Err(format!(
                        "{} live version {} is below minimum {}",
                        entry.id, version, entry.minimum_version
                    ));
                }
                if version > &entry.last_compatible_version {
                    return Err(format!(
                        "{} live version {} is newer than compatible version {}",
                        entry.id, version, entry.last_compatible_version
                    ));
                }
                parse_timestamp(timestamp, "liveVerifiedAt")?;
            }
            _ => {
                return Err(format!(
                    "{} live evidence must include both version and timestamp",
                    entry.id
                ));
            }
        }
    }
    Ok(())
}

fn parse_timestamp(value: &str, field: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| format!("{field} must be a valid RFC3339 timestamp"))
}

#[cfg(test)]
mod tests {
    use super::{bundled_compatibility_manifest, validate_embedded_manifest};

    #[test]
    fn manifest_validation_rejects_duplicate_harnesses() {
        let mut manifest = bundled_compatibility_manifest().expect("embedded manifest");
        let duplicate = manifest.harnesses[0].clone();
        let duplicate_id = duplicate.id;
        manifest.harnesses.push(duplicate);

        assert_eq!(
            validate_embedded_manifest(&manifest),
            Err(format!("duplicate harness entry for {duplicate_id}"))
        );
    }

    #[test]
    fn manifest_validation_rejects_incomplete_live_evidence() {
        let mut manifest = bundled_compatibility_manifest().expect("embedded manifest");
        let entry = &mut manifest.harnesses[0];
        let harness = entry.id;
        entry.live_verified_at = None;

        assert_eq!(
            validate_embedded_manifest(&manifest),
            Err(format!(
                "{harness} live evidence must include both version and timestamp"
            ))
        );
    }
}
