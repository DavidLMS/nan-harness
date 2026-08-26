use super::PrivatePathKind;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use winapi::um::winnt::{GENERIC_READ, GENERIC_WRITE, WRITE_DAC};
use windows_permissions::constants::{SeObjectType, SecurityInformation};
use windows_permissions::wrappers;
use windows_permissions::{LocalBox, SecurityDescriptor};

const PRIVATE_FILE_ACCESS: u32 = GENERIC_READ | GENERIC_WRITE | WRITE_DAC;

fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .access_mode(PRIVATE_FILE_ACCESS);
    options
}

pub(super) fn open_new(path: &Path) -> io::Result<File> {
    private_file_options().create_new(true).open(path)
}

pub(super) fn open_truncate(path: &Path) -> io::Result<File> {
    private_file_options()
        .create(true)
        .truncate(true)
        .open(path)
}

fn security_descriptor(kind: PrivatePathKind) -> io::Result<LocalBox<SecurityDescriptor>> {
    let user_sid = windows_permissions::utilities::current_process_sid()?;
    let inheritance = match kind {
        PrivatePathKind::File => "",
        PrivatePathKind::Directory => "OICI",
    };
    let sddl = format!("D:P(A;{inheritance};FA;;;{user_sid})(A;{inheritance};FA;;;SY)");
    sddl.parse()
}

fn apply_to_handle<H: AsRawHandle>(handle: &mut H, kind: PrivatePathKind) -> io::Result<()> {
    let descriptor = security_descriptor(kind)?;
    let dacl = descriptor.dacl().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "private filesystem descriptor has no DACL",
        )
    })?;

    wrappers::SetSecurityInfo(
        handle,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        Some(dacl),
        None,
    )
}

fn apply_to_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    let descriptor = security_descriptor(kind)?;
    let dacl = descriptor.dacl().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "private filesystem descriptor has no DACL",
        )
    })?;

    wrappers::SetNamedSecurityInfo(
        path,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        Some(dacl),
        None,
    )
}

pub(super) fn restrict_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    apply_to_path(path, kind)
}

pub(super) fn restrict_file(file: &mut File) -> io::Result<()> {
    apply_to_handle(file, PrivatePathKind::File)
}
