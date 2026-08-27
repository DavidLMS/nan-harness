use super::PrivatePathKind;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use winapi::um::winnt::{GENERIC_READ, GENERIC_WRITE, WRITE_DAC};
use windows_permissions::constants::{
    AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation,
};
use windows_permissions::wrappers;
use windows_permissions::{LocalBox, SecurityDescriptor, Sid};

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

fn postcondition_failed(reason: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("private filesystem DACL verification failed: {reason}"),
    )
}

fn parse_dacl_header(sddl: &str) -> Option<&str> {
    let mut remaining = sddl.strip_prefix("D:")?;
    let mut saw_protected = false;
    let mut saw_auto_inherit_request = false;
    let mut saw_auto_inherited = false;

    loop {
        if remaining.starts_with('(') {
            return saw_protected.then_some(remaining);
        }

        if remaining.starts_with('P') {
            if saw_protected {
                return None;
            }
            saw_protected = true;
            remaining = &remaining[1..];
            continue;
        }

        let flag = remaining.get(..2)?;
        match flag {
            "AR" if !saw_auto_inherit_request => saw_auto_inherit_request = true,
            "AI" if !saw_auto_inherited => saw_auto_inherited = true,
            _ => return None,
        }
        remaining = &remaining[2..];
    }
}

fn verify_dacl_sddl(descriptor: &SecurityDescriptor, kind: PrivatePathKind) -> io::Result<()> {
    let sddl = wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
        descriptor,
        SecurityInformation::Dacl,
    )
    .map_err(|_| postcondition_failed("could not read the security descriptor"))?;
    let sddl = sddl.to_string_lossy();
    let dacl = parse_dacl_header(&sddl)
        .ok_or_else(|| postcondition_failed("DACL is not protected with the exact contract"))?;
    let expected_flags = match kind {
        PrivatePathKind::File => "",
        PrivatePathKind::Directory => "OICI",
    };

    let mut remaining = dacl;
    let mut ace_count = 0;
    while !remaining.is_empty() {
        let body = remaining
            .strip_prefix('(')
            .and_then(|remaining| remaining.split_once(')'))
            .ok_or_else(|| postcondition_failed("DACL contains malformed ACE metadata"))?;
        remaining = body.1;
        let fields = body.0.split(';').collect::<Vec<_>>();
        if fields.len() != 6
            || fields[0] != "A"
            || fields[1] != expected_flags
            || fields[2] != "FA"
            || !fields[3].is_empty()
            || !fields[4].is_empty()
        {
            return Err(postcondition_failed(
                "DACL contains unexpected ACE control or inheritance flags",
            ));
        }
        ace_count += 1;
    }

    if ace_count != 2 {
        return Err(postcondition_failed(
            "DACL does not contain exactly two ACE descriptors",
        ));
    }

    Ok(())
}

fn verify_descriptor(descriptor: &SecurityDescriptor, kind: PrivatePathKind) -> io::Result<()> {
    verify_dacl_sddl(descriptor, kind)?;

    let dacl = descriptor
        .dacl()
        .ok_or_else(|| postcondition_failed("security descriptor has no DACL"))?;
    if dacl.len() != 2 {
        return Err(postcondition_failed(
            "DACL does not contain exactly two entries",
        ));
    }

    let user_sid = windows_permissions::utilities::current_process_sid()
        .map_err(|_| postcondition_failed("could not resolve the current process SID"))?;
    let system_sid: LocalBox<Sid> = "S-1-5-18"
        .parse()
        .map_err(|_| postcondition_failed("could not resolve the SYSTEM SID"))?;
    let expected_flags = match kind {
        PrivatePathKind::File => AceFlags::empty(),
        PrivatePathKind::Directory => AceFlags::ContainerInherit | AceFlags::ObjectInherit,
    };

    let mut saw_user = false;
    let mut saw_system = false;
    for index in 0..dacl.len() {
        let ace = dacl
            .get_ace(index)
            .ok_or_else(|| postcondition_failed("DACL entry could not be read"))?;
        if ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE
            || ace.mask() != AccessRights::FileAllAccess
            || ace.flags() != expected_flags
        {
            return Err(postcondition_failed(
                "DACL contains an unexpected access entry",
            ));
        }

        let sid = ace
            .sid()
            .ok_or_else(|| postcondition_failed("DACL entry has no principal"))?;
        if sid == &*user_sid {
            if saw_user {
                return Err(postcondition_failed("DACL contains a duplicate user entry"));
            }
            saw_user = true;
        } else if sid == &*system_sid {
            if saw_system {
                return Err(postcondition_failed(
                    "DACL contains a duplicate SYSTEM entry",
                ));
            }
            saw_system = true;
        } else {
            return Err(postcondition_failed(
                "DACL contains a principal outside the private-filesystem contract",
            ));
        }
    }

    if !saw_user || !saw_system {
        return Err(postcondition_failed(
            "DACL is missing the current user or SYSTEM entry",
        ));
    }

    Ok(())
}

fn verify_handle<H: AsRawHandle>(handle: &H, kind: PrivatePathKind) -> io::Result<()> {
    let descriptor = wrappers::GetSecurityInfo(
        handle,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )
    .map_err(|_| postcondition_failed("could not read the hardened file handle"))?;
    verify_descriptor(&descriptor, kind)
}

fn verify_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    let descriptor = wrappers::GetNamedSecurityInfo(
        path,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl,
    )
    .map_err(|_| postcondition_failed("could not read the hardened filesystem path"))?;
    verify_descriptor(&descriptor, kind)
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
    )?;

    verify_handle(handle, kind)
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
    )?;

    verify_path(path, kind)
}

pub(super) fn restrict_path(path: &Path, kind: PrivatePathKind) -> io::Result<()> {
    apply_to_path(path, kind)
}

pub(super) fn restrict_file(file: &mut File) -> io::Result<()> {
    apply_to_handle(file, PrivatePathKind::File)
}

#[cfg(test)]
mod tests {
    use super::parse_dacl_header;

    #[test]
    fn parses_protected_dacl_headers_with_supported_auto_inheritance_flags() {
        for (sddl, expected_aces) in [
            ("D:P(A;;FA;;;SY)", "(A;;FA;;;SY)"),
            ("D:PAR(A;;FA;;;SY)", "(A;;FA;;;SY)"),
            ("D:PAI(A;;FA;;;SY)", "(A;;FA;;;SY)"),
            ("D:PARAI(A;;FA;;;SY)", "(A;;FA;;;SY)"),
            ("D:ARP(A;;FA;;;SY)", "(A;;FA;;;SY)"),
            ("D:AIPAR(A;;FA;;;SY)", "(A;;FA;;;SY)"),
        ] {
            assert_eq!(parse_dacl_header(sddl), Some(expected_aces));
        }
    }

    #[test]
    fn rejects_unprotected_unknown_or_malformed_dacl_headers() {
        for sddl in [
            "D:(A;;FA;;;SY)",
            "D:AI(A;;FA;;;SY)",
            "D:AR(A;;FA;;;SY)",
            "D:PX(A;;FA;;;SY)",
            "D:PAX(A;;FA;;;SY)",
            "D:PAAI(A;;FA;;;SY)",
            "D:PP(A;;FA;;;SY)",
            "D:PAIAI(A;;FA;;;SY)",
            "D:PARAR(A;;FA;;;SY)",
            "D:P",
            "D:PAI",
            "D:Pnot-an-ace",
        ] {
            assert_eq!(
                parse_dacl_header(sddl),
                None,
                "unexpectedly accepted {sddl}"
            );
        }
    }
}
