//! Read only the root package.json from Electron's length-prefixed ASAR format.
//! Layout follows electron/asar's src/disk.ts; archive code is never evaluated.

use super::{DiscoveryError, Version, parse_version};
use std::fs::File;
use std::io::{Read, Seek as _, SeekFrom};
use std::path::Path;

const MAX_PACKAGE_BYTES: u64 = 65_536;

pub(super) fn version(path: &Path) -> Result<Option<Version>, DiscoveryError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DiscoveryError::Unreadable),
    };
    let length = file
        .metadata()
        .map_err(|_| DiscoveryError::Unreadable)?
        .len();
    let Some((start, size, _)) = package_location(&mut file, length)? else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|_| DiscoveryError::Unreadable)?;
    let package = read_package(&mut file, size)?;
    Ok(package_version(&package).and_then(|version| parse_version(&version)))
}

/// Stream variant for archives that cannot seek; bytes before package.json are discarded.
pub(crate) fn package_json(
    reader: &mut impl Read,
    length: u64,
) -> Result<Option<Vec<u8>>, DiscoveryError> {
    let Some((start, size, consumed)) = package_location(reader, length)? else {
        return Ok(None);
    };
    let skip = start
        .checked_sub(consumed)
        .ok_or(DiscoveryError::Unreadable)?;
    let skipped = std::io::copy(&mut reader.take(skip), &mut std::io::sink())
        .map_err(|_| DiscoveryError::Unreadable)?;
    if skipped != skip {
        return Err(DiscoveryError::Unreadable);
    }
    read_package(reader, size).map(Some)
}

/// The raw `version` string of a bounded package.json document.
pub(crate) fn package_version(package: &[u8]) -> Option<String> {
    let package: serde_json::Value = serde_json::from_slice(package).ok()?;
    package
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Returns package.json's absolute start, its size, and the bytes already consumed.
fn package_location(
    reader: &mut impl Read,
    length: u64,
) -> Result<Option<(u64, u64, u64)>, DiscoveryError> {
    let mut prefix = [0u8; 16];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| DiscoveryError::Unreadable)?;
    let word =
        |start| u32::from_le_bytes(prefix[start..start + 4].try_into().expect("four-byte word"));
    let header_size = u64::from(word(4));
    let json_size = u64::from(word(12));
    if word(0) != 4
        || !(8..=16 * 1024 * 1024).contains(&header_size)
        || header_size + 8 > length
        || json_size > header_size - 8
    {
        return Err(DiscoveryError::Unreadable);
    }
    let mut bytes = vec![0u8; usize::try_from(json_size).map_err(|_| DiscoveryError::Unreadable)?];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| DiscoveryError::Unreadable)?;
    let header: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| DiscoveryError::Unreadable)?;
    let Some(package) = header
        .get("files")
        .and_then(|files| files.get("package.json"))
    else {
        return Ok(None);
    };
    if package.get("unpacked").and_then(serde_json::Value::as_bool) == Some(true)
        || package.get("link").is_some()
    {
        return Ok(None);
    }
    let size = package
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .ok_or(DiscoveryError::Unreadable)?;
    let offset = package
        .get("offset")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(DiscoveryError::Unreadable)?;
    let start = header_size
        .checked_add(8)
        .and_then(|start| start.checked_add(offset))
        .ok_or(DiscoveryError::Unreadable)?;
    if size > MAX_PACKAGE_BYTES || start.checked_add(size).is_none_or(|end| end > length) {
        return Err(DiscoveryError::Unreadable);
    }
    Ok(Some((start, size, 16 + json_size)))
}

fn read_package(reader: &mut impl Read, size: u64) -> Result<Vec<u8>, DiscoveryError> {
    let mut bytes = vec![0u8; usize::try_from(size).map_err(|_| DiscoveryError::Unreadable)?];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| DiscoveryError::Unreadable)?;
    Ok(bytes)
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

#[cfg(test)]
mod tests {
    use super::{Version, fixture, package_json, package_version, version};

    #[test]
    fn reads_bounded_package_metadata_without_running_electron() {
        let mut bytes = fixture("1.2.8");
        let root = tempfile::tempdir().expect("fixture");
        let path = root.path().join("app.asar");
        std::fs::write(&path, &bytes).expect("asar fixture");
        assert_eq!(version(&path), Ok(Version::parse("1.2.8").ok()));
        let length = bytes.len() as u64;
        let package = package_json(&mut bytes.as_slice(), length)
            .unwrap()
            .unwrap();
        assert_eq!(package_version(&package).as_deref(), Some("1.2.8"));
        bytes.truncate(20);
        std::fs::write(&path, bytes).expect("truncated fixture");
        assert!(version(&path).is_err());
    }
}
