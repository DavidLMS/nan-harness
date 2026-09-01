use super::*;

pub(crate) fn block_range(
    source: &str,
    begin: &str,
    end: &str,
) -> Result<Option<std::ops::Range<usize>>, ConfigurationError> {
    let starts = source.match_indices(begin).collect::<Vec<_>>();
    let ends = source.match_indices(end).collect::<Vec<_>>();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, _)], [(end_start, _)]) if start < end_start => {
            let mut end_index = end_start + end.len();
            if source.as_bytes().get(end_index) == Some(&b'\n') {
                end_index += 1;
            }
            Ok(Some(*start..end_index))
        }
        _ => Err(ConfigurationError::InvalidManagedBlock),
    }
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ConfigurationError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigurationError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn file_permissions(path: &Path) -> Result<Option<Permissions>, ConfigurationError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigurationError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn remove_optional_file(path: &Path) -> Result<(), ConfigurationError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigurationError::RemoveDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn hash_json(value: &Value) -> Result<String, ConfigurationError> {
    serde_json::to_vec(value)
        .map(|payload| sha256(&payload))
        .map_err(ConfigurationError::SerializeDocument)
}

pub(crate) fn sha256(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn yaml_quote(value: &str) -> Result<String, ConfigurationError> {
    serde_json::to_string(value).map_err(ConfigurationError::SerializeDocument)
}

pub(crate) fn dotenv_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
