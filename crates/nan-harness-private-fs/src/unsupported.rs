use super::{PrivateFileReadStatus, PrivatePathKind};
use std::fs::File;
use std::io;
use std::path::Path;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "private filesystem permissions are unsupported on this platform",
    )
}

pub(super) fn create_private_dir(_path: &Path) -> io::Result<()> {
    Err(unsupported())
}

pub(super) fn open_new(_path: &Path) -> io::Result<File> {
    Err(unsupported())
}

pub(super) fn open_truncate(_path: &Path) -> io::Result<File> {
    Err(unsupported())
}

pub(super) fn open_private_read(_path: &Path) -> io::Result<(File, PrivateFileReadStatus)> {
    Err(unsupported())
}

pub(super) fn restrict_path(_path: &Path, _kind: PrivatePathKind) -> io::Result<()> {
    Err(unsupported())
}

pub(super) fn restrict_file(_file: &mut File) -> io::Result<()> {
    Err(unsupported())
}
