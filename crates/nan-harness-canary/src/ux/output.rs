use super::errors::UxError;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;

pub(super) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), UxError> {
    let parent = path
        .parent()
        .ok_or_else(|| UxError::InvalidOutputPath(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| UxError::CreateOutput {
        path: parent.to_owned(),
        source,
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-harness-ux-")
        .tempfile_in(parent)
        .map_err(|source| UxError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| UxError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| UxError::WriteOutput {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::atomic_write;

    #[test]
    fn atomic_write_replaces_the_complete_output() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("nested/catalog.html");
        std::fs::create_dir_all(path.parent().expect("output should have a parent"))
            .expect("output directory should exist");
        std::fs::write(&path, b"stale trailing contents").expect("old output should be written");

        atomic_write(&path, b"new contents").expect("output should be replaced");

        assert_eq!(
            std::fs::read_to_string(path).expect("output should be readable"),
            "new contents"
        );
    }
}
