//! Retained filesystem handles keep the MCP tool independent of expected output.
use std::{
    fs::{File, Metadata, OpenOptions},
    io::{self, Read as _, Seek as _},
    path::{Path, PathBuf},
};

pub(super) struct ReadTarget {
    path: PathBuf,
    root: PathBuf,
    directory: File,
    file: File,
    original: Metadata,
}

fn rejected() -> io::Error {
    io::Error::other("Owned fixture custody rejected")
}

impl ReadTarget {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        let root = path.parent().ok_or_else(rejected)?;
        if !path.is_absolute()
            || path.file_name().and_then(|name| name.to_str()) != Some("read-target.txt")
            || path.canonicalize()? != path
            || root.canonicalize()? != root
            || !std::fs::symlink_metadata(path)?.is_file()
        {
            return Err(rejected());
        }
        let directory = open_retained(root, true)?;
        let file = open_retained(path, false)?;
        let original = file.metadata()?;
        let mut target = Self {
            path: path.into(),
            root: root.into(),
            directory,
            file,
            original,
        };
        target.read()?;
        Ok(target)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn owns(&self, path: &Path) -> bool {
        path == self.path && self.validate().is_ok()
    }

    fn validate(&self) -> io::Result<()> {
        if self.root.canonicalize()? != self.root
            || self.path.canonicalize()? != self.path
            || !regular_private(&self.directory, true)?
            || !regular_private(&self.file, false)?
            || !same_file(&self.original, &self.file.metadata()?)
            || !same_file(
                &self.file.metadata()?,
                &std::fs::symlink_metadata(&self.path)?,
            )
            || !same_file(
                &self.directory.metadata()?,
                &std::fs::symlink_metadata(&self.root)?,
            )
            || self.original.len() > 4096
        {
            return Err(rejected());
        }
        Ok(())
    }

    pub(super) fn read(&mut self) -> io::Result<String> {
        self.validate()?;
        self.file.rewind()?;
        let mut text = String::new();
        (&mut self.file).take(4097).read_to_string(&mut text)?;
        self.validate()?;
        if text.len() > 4096 {
            return Err(rejected());
        }
        Ok(text)
    }
}

#[cfg(windows)]
fn open_retained(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    // OPEN_REPARSE_POINT; directory handles additionally need BACKUP_SEMANTICS.
    // Withhold delete sharing on both handles and write sharing on the target.
    OpenOptions::new()
        .read(true)
        .access_mode(0x8002_0000)
        .share_mode(if directory { 3 } else { 1 })
        .custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 })
        .open(path)
}

#[cfg(unix)]
fn open_retained(path: &Path, _directory: bool) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn regular_private(file: &File, directory: bool) -> io::Result<bool> {
    use nan_harness_private_fs::{OwnedWindowsDacl, PrivatePathKind, classify_owned_windows_dacl};
    use std::os::windows::fs::MetadataExt as _;
    let metadata = file.metadata()?;
    Ok(metadata.file_attributes() & 0x400 == 0
        && (if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        })
        && classify_owned_windows_dacl(
            file,
            if directory {
                PrivatePathKind::Directory
            } else {
                PrivatePathKind::File
            },
        ) == OwnedWindowsDacl::Protected)
}

#[cfg(unix)]
fn regular_private(file: &File, directory: bool) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = file.metadata()?;
    Ok(metadata.mode().trailing_zeros() >= 6
        && metadata.uid() == nix::unistd::geteuid().as_raw()
        && (if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        }))
}

#[cfg(unix)]
fn same_file(before: &Metadata, after: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && (before.is_dir()
            || (before.len() == after.len()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()))
}

#[cfg(windows)]
fn same_file(before: &Metadata, after: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    // Retained handles deny replacement; timestamps/attributes detect changes
    // to the path while the file and its immediate directory remain locked.
    before.creation_time() == after.creation_time()
        && before.file_attributes() == after.file_attributes()
        && (before.is_dir()
            || (before.file_size() == after.file_size()
                && before.last_write_time() == after.last_write_time()))
}
