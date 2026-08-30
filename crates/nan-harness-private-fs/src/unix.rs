use super::{PrivateFileReadStatus, PrivatePathKind};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

pub(super) fn open_new(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

pub(super) fn open_truncate(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

pub(super) fn open_private_read(path: &Path) -> io::Result<(File, PrivateFileReadStatus)> {
    let file = OpenOptions::new().read(true).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private read target is not a file",
        ));
    }
    let is_private = metadata.permissions().mode().trailing_zeros() >= 6;
    super::finish_private_read(file, is_private, restrict_file)
}

pub(super) fn restrict_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    let mode = match kind {
        PrivatePathKind::File => 0o600,
        PrivatePathKind::Directory => 0o700,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

pub(super) fn restrict_file(file: &mut File) -> io::Result<()> {
    file.set_permissions(fs::Permissions::from_mode(0o600))
}
