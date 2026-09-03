use super::errors::AggregateError;
use serde::Serialize;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;

pub(super) fn atomic_json_write(path: &Path, value: &impl Serialize) -> Result<(), AggregateError> {
    let parent = path
        .parent()
        .ok_or_else(|| AggregateError::InvalidOutputPath(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| AggregateError::CreateDirectory {
        path: parent.to_owned(),
        source,
    })?;
    let payload = serde_json::to_vec_pretty(value).map_err(AggregateError::Serialize)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-harness-canary-aggregate-")
        .tempfile_in(parent)
        .map_err(|source| AggregateError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .write_all(&payload)
        .and_then(|()| temporary.write_all(b"\n"))
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| AggregateError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| AggregateError::WriteOutput {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::atomic_json_write;

    #[test]
    fn atomic_write_replaces_the_complete_json_document() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("summary.json");
        std::fs::write(&path, b"stale trailing contents")
            .expect("existing output should be written");

        atomic_json_write(&path, &serde_json::json!({ "schemaVersion": 2 }))
            .expect("output should be replaced");

        assert_eq!(
            std::fs::read_to_string(path).expect("output should be readable"),
            "{\n  \"schemaVersion\": 2\n}\n"
        );
    }
}
