use nan_harness_private_fs::open_private_truncate;
use std::fs::File;
use std::path::Path;

pub(crate) fn create(path: &Path) -> std::io::Result<File> {
    open_private_truncate(path)
}
