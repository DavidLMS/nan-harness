#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;
#[cfg(not(any(unix, windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Filesystem object type used to select the private DACL or Unix mode.
pub enum PrivatePathKind {
    /// A regular file, protected from inheritance.
    File,
    /// A directory, protected while allowing private descendants to inherit.
    Directory,
}

/// Restrict a filesystem path to the current user and the platform system principal.
///
/// On Unix this applies the owner-only mode associated with `kind`. On Windows this
/// replaces the DACL with a protected owner-and-`SYSTEM` DACL.
///
/// # Errors
///
/// Returns the platform I/O error if the path cannot be hardened.
pub fn restrict_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::restrict_path(path, kind)
    }

    #[cfg(windows)]
    {
        windows::restrict_path(path, kind)
    }

    #[cfg(not(any(unix, windows)))]
    {
        unsupported::restrict_path(path, kind)
    }
}

/// Restrict an open file to the current user and the platform system principal.
///
/// # Errors
///
/// Returns the platform I/O error if the file cannot be hardened.
pub fn restrict_file(file: &mut File) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::restrict_file(file)
    }

    #[cfg(windows)]
    {
        windows::restrict_file(file)
    }

    #[cfg(not(any(unix, windows)))]
    {
        unsupported::restrict_file(file)
    }
}

/// Exclusively create and harden a new private file before returning its handle.
///
/// If hardening fails, the newly created empty file is closed and removed on a
/// best-effort basis, and the original hardening error is returned.
///
/// # Errors
///
/// Returns the open or hardening error. Existing paths are never overwritten.
pub fn open_private_new(path: &Path) -> io::Result<File> {
    let mut file = open_new(path)?;

    if let Err(error) = restrict_file(&mut file) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }

    Ok(file)
}

/// Create or truncate a private file and harden it before returning its handle.
///
/// If hardening fails, the file may already have been truncated, but no caller
/// payload has been written through the returned handle.
///
/// # Errors
///
/// Returns the open or hardening error. The handle is not returned unless the
/// private-filesystem guarantee has been applied.
pub fn open_private_truncate(path: &Path) -> io::Result<File> {
    let mut file = open_truncate(path)?;
    restrict_file(&mut file)?;
    Ok(file)
}

fn open_new(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        unix::open_new(path)
    }

    #[cfg(windows)]
    {
        windows::open_new(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        unsupported::open_new(path)
    }
}

fn open_truncate(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        unix::open_truncate(path)
    }

    #[cfg(windows)]
    {
        windows::open_truncate(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        unsupported::open_truncate(path)
    }
}
