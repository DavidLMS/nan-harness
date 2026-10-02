//! Shared bounded package metadata for installed and frozen desktop releases.
use super::DiscoveryError;
pub(crate) use nan_harness_core::desktop_metadata::package_version;
use semver::Version;
use std::io::Read;
use std::path::Path;

pub(super) fn version(path: &Path) -> Result<Option<Version>, DiscoveryError> {
    nan_harness_core::desktop_metadata::version(path).map_err(|_| DiscoveryError::VersionResource)
}

pub(crate) fn package_json(
    reader: &mut impl Read,
    length: u64,
) -> Result<Option<Vec<u8>>, DiscoveryError> {
    nan_harness_core::desktop_metadata::package_json(reader, length)
        .map_err(|_| DiscoveryError::VersionResource)
}

#[cfg(test)]
pub(crate) fn fixture(version: &str) -> Vec<u8> {
    let package = format!(r#"{{"version":"{version}"}}"#).into_bytes();
    let filler = b"not package metadata";
    let header = serde_json::to_vec(&serde_json::json!({"files":{
        "index.js":{"size":filler.len(),"offset":"0"},
        "package.json":{"size":package.len(),"offset":filler.len().to_string()}
    }}))
    .expect("header");
    let header_len = u32::try_from(header.len()).expect("small fixture");
    let mut bytes = Vec::new();
    for value in [4u32, header_len + 8, header_len + 4, header_len] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(header);
    bytes.extend(filler);
    bytes.extend(package);
    bytes
}
