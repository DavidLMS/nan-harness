//! Read only the root package.json from Electron's length-prefixed ASAR format.
//! Layout follows electron/asar's src/disk.ts; archive code is never evaluated.

use super::{DiscoveryError, Version, parse_version};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

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
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix)
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
    file.read_exact(&mut bytes)
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
    if size > 65_536 || start.checked_add(size).is_none_or(|end| end > length) {
        return Err(DiscoveryError::Unreadable);
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|_| DiscoveryError::Unreadable)?;
    bytes.resize(
        usize::try_from(size).map_err(|_| DiscoveryError::Unreadable)?,
        0,
    );
    file.read_exact(&mut bytes)
        .map_err(|_| DiscoveryError::Unreadable)?;
    let package: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| DiscoveryError::Unreadable)?;
    Ok(package
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_version))
}

#[cfg(test)]
mod tests {
    use super::{Version, version};

    #[test]
    fn reads_bounded_package_metadata_without_running_electron() {
        let package = br#"{"version":"1.2.8"}"#;
        let header = serde_json::to_vec(
            &serde_json::json!({"files":{"package.json":{"size":package.len(),"offset":"0"}}}),
        )
        .expect("header");
        let header_len = u32::try_from(header.len()).expect("small fixture");
        let mut bytes = Vec::new();
        for value in [4u32, header_len + 8, header_len + 4, header_len] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(header);
        bytes.extend(package);
        let root = tempfile::tempdir().expect("fixture");
        let path = root.path().join("app.asar");
        std::fs::write(&path, &bytes).expect("asar fixture");
        assert_eq!(version(&path), Ok(Version::parse("1.2.8").ok()));
        bytes.truncate(20);
        std::fs::write(&path, bytes).expect("truncated fixture");
        assert!(version(&path).is_err());
    }
}
