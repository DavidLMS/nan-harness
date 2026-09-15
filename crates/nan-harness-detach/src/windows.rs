//! Windows implementation of the standard-handle release.
//!
//! This module is the single audited exception to the workspace `unsafe_code` rule. It calls
//! `GetStdHandle`, `SetStdHandle`, and `CloseHandle` because the standard library offers no way to
//! release a handle that a process inherited from its launcher, and because a detached helper must
//! not keep another process's pipes open.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
};

/// The null devices below stay open for the whole process lifetime, so a replaced standard handle
/// keeps pointing at a valid device.
static NULL_DEVICES: OnceLock<[File; 3]> = OnceLock::new();

/// Replaces this process's standard handles with the null device and closes the handles the
/// process inherited from its launcher.
///
/// # Errors
///
/// Returns the underlying I/O error when the null device cannot be opened or one of the standard
/// handles cannot be replaced.
pub fn release_inherited_standard_handles() -> io::Result<()> {
    let replacements = if let Some(replacements) = NULL_DEVICES.get() {
        replacements
    } else {
        let opened = [
            null_device(true, false)?,
            null_device(false, true)?,
            null_device(false, true)?,
        ];
        // A losing racing caller discarded an equivalent set of devices.
        let _ = NULL_DEVICES.set(opened);
        NULL_DEVICES
            .get()
            .ok_or_else(|| io::Error::other("the null devices were not stored for this process"))?
    };
    for (identifier, replacement) in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
        .into_iter()
        .zip(replacements.iter())
    {
        replace(identifier, replacement.as_raw_handle() as HANDLE)?;
    }
    Ok(())
}

fn null_device(read: bool, write: bool) -> io::Result<File> {
    OpenOptions::new().read(read).write(write).open("NUL")
}

fn replace(identifier: u32, replacement: HANDLE) -> io::Result<()> {
    // SAFETY: the three calls only exchange process-local handle values. `CloseHandle` releases the
    // handle this process inherited from its launcher, the replacement stays alive in
    // `NULL_DEVICES` for the whole process lifetime, and a standard handle that was never set is
    // left untouched.
    unsafe {
        let previous = GetStdHandle(identifier);
        if previous != INVALID_HANDLE_VALUE && !previous.is_null() && previous != replacement {
            CloseHandle(previous);
        }
        if SetStdHandle(identifier, replacement) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
