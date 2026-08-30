#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

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

/// Create a private directory without exposing it with permissive defaults.
///
/// On Windows the directory remains empty until its exact private DACL has been
/// applied and verified. If hardening fails, the empty directory is removed on
/// a best-effort basis.
///
/// # Errors
///
/// Returns the create or hardening error. Existing paths are never changed.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::create_private_dir(path)
    }

    #[cfg(windows)]
    {
        windows::create_private_dir(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        unsupported::create_private_dir(path)
    }
}

/// Create every missing directory in a path with private protection.
///
/// Existing directories are accepted without changing their permissions. If a
/// concurrent creator wins a race, the path is accepted only after metadata
/// confirms that it is a directory.
///
/// # Errors
///
/// Returns an I/O error when a component is not a directory or a missing
/// component cannot be created privately. Private ancestors created before a
/// later failure may remain.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::metadata(&current) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(not_a_directory(&current)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match create_private_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        match fs::metadata(&current) {
                            Ok(metadata) if metadata.is_dir() => {}
                            Ok(_) => return Err(not_a_directory(&current)),
                            Err(error) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn not_a_directory(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("path component '{}' is not a directory", path.display()),
    )
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
