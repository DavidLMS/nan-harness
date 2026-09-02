mod artifacts;
mod compatibility;
mod validation;
mod verification;
mod versioning;

pub(crate) use artifacts::generate_metadata;
pub(crate) use compatibility::{
    generate_compatibility_feed, merge_compatibility_feed, validate_compatibility_feed,
};
pub(crate) use validation::{validate_changelog, validate_tag, write_changelog_notes};
pub(crate) use versioning::set_version;

#[cfg(test)]
use artifacts::{AUXILIARY_ARTIFACTS, RELEASE_TARGETS, artifact_file_name};
#[cfg(test)]
use compatibility::{COMPATIBILITY_FILE_NAME, bundled_compatibility_manifest};
#[cfg(test)]
use validation::CITATION_FILE_NAME;
#[cfg(test)]
use verification::{
    HarnessRequirement, VerificationEntry, VerificationRelease, current_release_version,
    merge_evidence_pair, merge_verification_entry, validate_releases,
};
#[cfg(test)]
use versioning::{
    CARGO_MANIFEST_FILES, LOCAL_PACKAGE_NAMES, replace_lockfile_version, replace_manifest_version,
};

#[cfg(test)]
mod tests {
    use super::{
        AUXILIARY_ARTIFACTS, CARGO_MANIFEST_FILES, CITATION_FILE_NAME, COMPATIBILITY_FILE_NAME,
        HarnessRequirement, LOCAL_PACKAGE_NAMES, RELEASE_TARGETS, VerificationEntry,
        VerificationRelease, artifact_file_name, bundled_compatibility_manifest,
        current_release_version, generate_compatibility_feed, generate_metadata,
        merge_compatibility_feed, merge_evidence_pair, merge_verification_entry,
        replace_lockfile_version, replace_manifest_version, validate_releases, validate_tag,
    };
    use nan_harness_core::HarnessKind;
    use semver::Version;
    use serde_json::Value;
    use std::fs;

    #[test]
    fn accepts_only_the_exact_workspace_release_tag() {
        assert!(validate_tag(&format!("v{}", env!("CARGO_PKG_VERSION"))).is_ok());
        assert!(validate_tag(env!("CARGO_PKG_VERSION")).is_err());
        assert!(validate_tag("v999.0.0").is_err());
    }

    #[test]
    fn creates_the_complete_release_contract() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        fs::write(directory.path().join("install.sh"), "installer")
            .expect("shell installer should exist");
        fs::write(directory.path().join("install.ps1"), "installer")
            .expect("PowerShell installer should exist");
        for target in RELEASE_TARGETS {
            fs::write(directory.path().join(artifact_file_name(target)), target)
                .expect("artifact should exist");
        }
        for artifact in AUXILIARY_ARTIFACTS {
            fs::write(directory.path().join(artifact), artifact)
                .expect("auxiliary artifact should exist");
        }

        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        generate_metadata(&tag, "DavidLMS/nan-harness", directory.path())
            .expect("metadata should be generated");

        let manifest: Value = serde_json::from_slice(
            &fs::read(directory.path().join("update-manifest.json"))
                .expect("manifest should exist"),
        )
        .expect("manifest should be valid JSON");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            manifest["artifacts"]
                .as_array()
                .expect("artifacts should be an array")
                .len(),
            RELEASE_TARGETS.len()
        );
        assert!(
            manifest["artifacts"]
                .as_array()
                .expect("artifacts should be an array")
                .iter()
                .all(|artifact| artifact["url"]
                    .as_str()
                    .is_some_and(|url| !url.contains("canary")))
        );
        let compatibility: Value = serde_json::from_slice(
            &fs::read(directory.path().join(COMPATIBILITY_FILE_NAME))
                .expect("compatibility manifest should exist"),
        )
        .expect("compatibility manifest should be valid JSON");
        assert_eq!(compatibility["schemaVersion"], 2);
        assert_eq!(
            compatibility["releases"][0]["verifications"][0]["compatibleAt"],
            "2026-08-29T00:00:00Z"
        );
        assert_eq!(
            compatibility["releases"][0]["verifications"]
                .as_array()
                .expect("verifications should be an array")
                .len(),
            15
        );

        let citation = fs::read_to_string(directory.path().join(CITATION_FILE_NAME))
            .expect("citation file should be generated");
        assert!(citation.contains(&format!("version: \"{}\"", env!("CARGO_PKG_VERSION"))));

        let checksums = fs::read_to_string(directory.path().join("SHA256SUMS"))
            .expect("checksums should exist");
        assert!(checksums.contains("  install.sh\n"));
        assert!(checksums.contains("  CITATION.cff\n"));
        assert!(checksums.contains("  compatibility.json\n"));
        assert!(checksums.contains("  LICENSE\n"));
        assert!(checksums.contains("  NOTICE.md\n"));
        assert!(checksums.contains("  update-manifest.json\n"));
        for artifact in AUXILIARY_ARTIFACTS {
            assert!(checksums.contains(&format!("  {artifact}\n")));
        }
    }

    #[test]
    fn version_updates_only_touch_workspace_and_local_packages() {
        assert!(CARGO_MANIFEST_FILES.contains(&"crates/nan-harness-private-fs/Cargo.toml"));
        assert!(LOCAL_PACKAGE_NAMES.contains(&"nan-harness-private-fs"));

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let manifest = directory.path().join("Cargo.toml");
        fs::write(
            &manifest,
            concat!(
                "[workspace.package]\n",
                "version = \"0.0.1\"\n",
                "\n",
                "[workspace.dependencies]\n",
                "nan-harness-core = { path = \"core\", version = \"0.0.1\" }\n",
                "nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.1\" }\n",
                "nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.1\" }\n",
                "unrelated = { version = \"0.0.1\" }\n",
                "\n",
                "[dependencies.nan-harness-runtime]\n",
                "path = \"runtime\"\n",
                "version = \"0.0.1\"\n",
            ),
        )
        .expect("manifest fixture should exist");

        replace_manifest_version(&manifest, "0.0.1", "0.0.2")
            .expect("manifest versions should update");
        let updated = fs::read_to_string(manifest).expect("updated manifest should be readable");

        assert!(updated.contains("version = \"0.0.2\""));
        assert!(updated.contains("nan-harness-core = { path = \"core\", version = \"0.0.2\" }"));
        assert!(
            updated.contains(
                "nan-harness-diagnostics = { path = \"diagnostics\", version = \"0.0.2\" }"
            )
        );
        assert!(updated.contains(
            "[dependencies.nan-harness-runtime]\npath = \"runtime\"\nversion = \"0.0.2\""
        ));
        assert!(
            updated.contains(
                "nan-harness-private-fs = { path = \"private-fs\", version = \"0.0.2\" }"
            )
        );
        assert!(updated.contains("unrelated = { version = \"0.0.1\" }"));

        let private_manifest = directory.path().join("private-fs/Cargo.toml");
        fs::create_dir_all(
            private_manifest
                .parent()
                .expect("fixture parent should exist"),
        )
        .expect("private filesystem fixture directory should exist");
        fs::write(
            &private_manifest,
            concat!(
                "[package]\n",
                "name = \"nan-harness-private-fs\"\n",
                "version = \"0.0.1\"\n",
                "\n",
                "[dev-dependencies]\n",
                "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.1\" }\n",
            ),
        )
        .expect("private filesystem manifest fixture should exist");

        replace_manifest_version(&private_manifest, "0.0.1", "0.0.2")
            .expect("private filesystem manifest versions should update");
        let private_updated =
            fs::read_to_string(&private_manifest).expect("private manifest should be readable");
        assert!(private_updated.contains("version = \"0.0.2\""));
        assert!(private_updated.contains(
            "nan-harness-test-support = { path = \"../test-support\", version = \"0.0.2\" }"
        ));

        let lockfile = directory.path().join("Cargo.lock");
        fs::write(
            &lockfile,
            concat!(
                "version = 4\n\n",
                "[[package]]\n",
                "name = \"nan-harness-private-fs\"\n",
                "version = \"0.0.1\"\n",
                "dependencies = [\n",
                " \"nan-harness-test-support\",\n",
                "]\n\n",
                "[[package]]\n",
                "name = \"nan-harness-test-support\"\n",
                "version = \"0.0.1\"\n\n",
                "[[package]]\n",
                "name = \"unrelated\"\n",
                "version = \"0.0.1\"\n",
            ),
        )
        .expect("lockfile fixture should exist");

        replace_lockfile_version(&lockfile, "0.0.1", "0.0.2")
            .expect("local package lockfile versions should update");
        let lock_updated =
            fs::read_to_string(lockfile).expect("updated lockfile should be readable");
        assert!(lock_updated.contains("name = \"nan-harness-private-fs\"\nversion = \"0.0.2\""));
        assert!(lock_updated.contains("name = \"nan-harness-test-support\"\nversion = \"0.0.2\""));
        assert!(lock_updated.contains("name = \"unrelated\"\nversion = \"0.0.1\""));
    }

    #[test]
    fn compatibility_merges_only_known_non_regressing_updates() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        generate_compatibility_feed(&base).expect("base feed should be generated");
        fs::write(
            updates.join("fx.json"),
            r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z","lastLiveVerifiedVersion":"0.0.4","liveVerifiedAt":"2026-08-20T08:00:00Z"}"#,
        )
        .expect("fx update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("compatibility feed should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        assert_eq!(merged["schemaVersion"], 2);
        let fx = merged["releases"][0]["verifications"]
            .as_array()
            .expect("verifications should be an array")
            .iter()
            .find(|entry| entry["id"] == "fx")
            .expect("fx should remain in the feed");

        assert_eq!(fx["lastCompatibleVersion"], "0.0.7");
        assert_eq!(fx["lastLiveVerifiedVersion"], "0.0.4");
        assert_eq!(fx["compatibleAt"], "2026-08-29T00:00:00Z");
    }

    #[test]
    fn compatibility_preserves_releases_and_merges_partial_updates_monotonically() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        fs::write(
            &base,
            format!(
                r#"{{"schemaVersion":2,"releases":[{{"nanHarnessVersion":"0.0.5","verifications":[{{"id":"fx","lastCompatibleVersion":"0.0.5","compatibleAt":"2026-08-01T00:00:00Z"}}]}},{{"nanHarnessVersion":"{}","verifications":[]}}]}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("base feed should exist");
        fs::write(
            updates.join("fx.json"),
            r#"{"id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-01T00:00:00Z"}"#,
        )
        .expect("partial update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("partial compatibility update should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        let releases = merged["releases"]
            .as_array()
            .expect("releases should be an array");
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0]["nanHarnessVersion"], "0.0.5");
        let current = releases
            .iter()
            .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
            .expect("current release should remain in the feed");
        assert!(current["verifications"].as_array().is_some());
    }

    #[test]
    fn compatibility_does_not_seed_a_new_release_before_an_update() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let base = directory.path().join("base.json");
        let updates = directory.path().join("updates");
        let output = directory.path().join("merged.json");
        fs::create_dir(&updates).expect("updates directory should exist");
        fs::write(
            &base,
            r#"{"schemaVersion":2,"releases":[{"nanHarnessVersion":"0.0.5","verifications":[]}]}"#,
        )
        .expect("base feed should exist");
        fs::write(
            updates.join("fx.json"),
            format!(
                r#"{{"nanHarnessVersion":"{}","id":"fx","lastCompatibleVersion":"0.0.4","compatibleAt":"2026-08-20T08:00:00Z"}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .expect("fx update should exist");

        merge_compatibility_feed(&base, &updates, &output)
            .expect("compatibility feed should merge");
        let merged: Value =
            serde_json::from_slice(&fs::read(output).expect("merged feed should be readable"))
                .expect("merged feed should be JSON");
        let current = merged["releases"]
            .as_array()
            .expect("releases should be an array")
            .iter()
            .find(|release| release["nanHarnessVersion"] == env!("CARGO_PKG_VERSION"))
            .expect("updated release should exist");
        assert_eq!(current["verifications"].as_array().unwrap().len(), 1);
        assert_eq!(current["verifications"][0]["id"], "fx");
    }

    #[test]
    fn compatibility_merge_rejects_malformed_pairs_and_missing_evidence() {
        let requirements = requirements();
        let cases = [
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
            },
            VerificationEntry {
                id: "codex".to_owned(),
                last_compatible_version: Some(Version::new(0, 147, 0)),
                compatible_at: Some("2026-08-19".to_owned()),
                last_live_verified_version: None,
                live_verified_at: None,
            },
        ];
        for entry in cases {
            let result = validate_releases(
                &[VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: vec![entry],
                }],
                &requirements,
                "test feed",
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn compatibility_merge_rejects_minimum_duplicate_and_live_order_violations() {
        let requirements = requirements();
        let below_minimum = entry("codex", "0.145.0", "2026-08-19T00:00:00Z");
        assert!(validate_single(&requirements, below_minimum).is_err());

        let duplicate = entry("codex", "0.147.0", "2026-08-19T00:00:00Z");
        assert!(
            validate_releases(
                &[VerificationRelease {
                    nan_harness_version: current_release_version(),
                    verifications: vec![duplicate.clone(), duplicate],
                }],
                &requirements,
                "test feed",
            )
            .is_err()
        );
        assert!(
            validate_releases(
                &[
                    VerificationRelease {
                        nan_harness_version: current_release_version(),
                        verifications: Vec::new(),
                    },
                    VerificationRelease {
                        nan_harness_version: current_release_version(),
                        verifications: Vec::new(),
                    },
                ],
                &requirements,
                "test feed",
            )
            .is_err()
        );

        let live_ahead = VerificationEntry {
            id: "codex".to_owned(),
            last_compatible_version: Some(Version::new(0, 146, 0)),
            compatible_at: Some("2026-08-19T00:00:00Z".to_owned()),
            last_live_verified_version: Some(Version::new(0, 147, 0)),
            live_verified_at: Some("2026-08-20T00:00:00Z".to_owned()),
        };
        assert!(validate_single(&requirements, live_ahead).is_err());
    }

    #[test]
    fn compatibility_merge_preserves_unknown_ids_and_merges_pairs_atomically() {
        let requirements = requirements();
        let unknown = entry("future-harness", "99.0.0", "2026-08-19T00:00:00Z");
        assert!(validate_single(&requirements, unknown.clone()).is_ok());

        let mut current = entry("fx", "0.0.3", "2026-08-20T00:00:00Z");
        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.4", "2026-08-19T00:00:00Z"),
            "test feed",
        )
        .expect("higher version should replace the complete pair");
        assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 4)));
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.5", "2026-08-18T00:00:00Z"),
            "test feed",
        )
        .expect("higher version should retain the newer existing timestamp");
        assert_eq!(current.last_compatible_version, Some(Version::new(0, 0, 5)));
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.4", "2026-08-20T00:00:00Z"),
            "test feed",
        )
        .expect("equal version with later timestamp should advance the timestamp");
        assert_eq!(
            current.compatible_at.as_deref(),
            Some("2026-08-20T00:00:00Z")
        );
        let unchanged = current.clone();
        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.3", "2026-08-21T00:00:00Z"),
            "test feed",
        )
        .expect("lower version should be ignored");
        assert_eq!(current, unchanged);

        merge_verification_entry(
            &mut current,
            &entry("fx", "0.0.5", "2026-08-19T00:00:00Z"),
            "test feed",
        )
        .expect("equal version with an older timestamp should be ignored");
        assert_eq!(current, unchanged);

        let mut absent_version = None;
        let mut stray_timestamp = Some("2026-08-21T00:00:00Z".to_owned());
        merge_evidence_pair(
            &mut absent_version,
            &mut stray_timestamp,
            None,
            Some(&"2026-08-22T00:00:00Z".to_owned()),
            "fx",
            "compatible",
            "test feed",
        )
        .expect("an update without a version should be ignored");
        assert_eq!(absent_version, None);
        assert_eq!(stray_timestamp.as_deref(), Some("2026-08-21T00:00:00Z"));

        let mut incomplete_version = Some(Version::new(0, 0, 3));
        let mut incomplete_timestamp = None;
        assert!(
            merge_evidence_pair(
                &mut incomplete_version,
                &mut incomplete_timestamp,
                Some(&Version::new(0, 0, 4)),
                Some(&"2026-08-22T00:00:00Z".to_owned()),
                "fx",
                "compatible",
                "test feed",
            )
            .is_err()
        );
    }

    fn requirements() -> std::collections::BTreeMap<HarnessKind, HarnessRequirement> {
        let manifest = bundled_compatibility_manifest().expect("embedded manifest");
        manifest
            .harnesses
            .into_iter()
            .map(|entry| {
                (
                    entry.id,
                    HarnessRequirement {
                        minimum_version: entry.minimum_version,
                        compatible_version: entry.last_compatible_version,
                    },
                )
            })
            .collect()
    }

    fn validate_single(
        requirements: &std::collections::BTreeMap<HarnessKind, HarnessRequirement>,
        entry: VerificationEntry,
    ) -> Result<(), String> {
        validate_releases(
            &[VerificationRelease {
                nan_harness_version: current_release_version(),
                verifications: vec![entry],
            }],
            requirements,
            "test feed",
        )
    }

    fn entry(id: &str, version: &str, timestamp: &str) -> VerificationEntry {
        VerificationEntry {
            id: id.to_owned(),
            last_compatible_version: Some(Version::parse(version).expect("version")),
            compatible_at: Some(timestamp.to_owned()),
            last_live_verified_version: None,
            live_verified_at: None,
        }
    }
}
