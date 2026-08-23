use nan_harness_core::HarnessKind;
use nan_harness_test_support::manifest::ConformanceManifest;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityManifest {
    harnesses: Vec<CompatibilityEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityEntry {
    id: HarnessKind,
    last_compatible_version: String,
}

#[test]
fn every_supported_harness_has_a_current_conformance_manifest() {
    let root = workspace_root();
    let compatibility: CompatibilityManifest = serde_json::from_slice(
        &std::fs::read(root.join("crates/nan-harness-runtime/resources/compatibility.json"))
            .expect("compatibility manifest should be readable"),
    )
    .expect("compatibility manifest should be valid");
    let versions = compatibility
        .harnesses
        .into_iter()
        .map(|entry| (entry.id, entry.last_compatible_version))
        .collect::<BTreeMap<_, _>>();

    for harness in HarnessKind::ALL {
        let path = root
            .join("tests/conformance")
            .join(harness.to_string())
            .join("manifest.toml");
        let manifest = ConformanceManifest::load(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(manifest.harness, harness);
        assert!(
            !manifest.tool_names().is_empty(),
            "{harness} must declare its tested tool or protocol surface"
        );
        assert_eq!(
            Some(&manifest.compatibility_version().to_owned()),
            versions.get(&harness),
            "{harness} conformance and compatibility versions must move together"
        );
        semver::Version::parse(manifest.compatibility_version())
            .unwrap_or_else(|error| panic!("{harness} has an invalid version: {error}"));
    }
}

#[test]
fn hermes_dynamic_inventory_rules_are_manifest_data() {
    let path = workspace_root().join("tests/conformance/hermes/manifest.toml");
    let manifest = ConformanceManifest::load(&path).expect("Hermes manifest should parse");
    assert_eq!(manifest.optional_inventory, vec!["computer_use"]);
    let variants = manifest
        .dynamic_inventory
        .iter()
        .map(|variant| variant.iter().cloned().collect::<BTreeSet<_>>())
        .collect::<Vec<_>>();
    assert!(variants.contains(&BTreeSet::new()));
    assert!(variants.contains(&BTreeSet::from(["browser_exec".to_owned()])));
    assert!(variants.iter().any(|variant| {
        variant.contains("browser_snapshot") && variant.contains("browser_type")
    }));
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("test support should be inside the workspace")
        .to_path_buf()
}
