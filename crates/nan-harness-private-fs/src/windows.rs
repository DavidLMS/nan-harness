use super::{PrivateFileReadStatus, PrivatePathKind};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use winapi::um::winnt::{GENERIC_READ, GENERIC_WRITE, READ_CONTROL, WRITE_DAC};
use windows_permissions::constants::{
    AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation,
};
use windows_permissions::wrappers;
use windows_permissions::{Acl, LocalBox, SecurityDescriptor, Sid};

const PRIVATE_FILE_ACCESS: u32 = GENERIC_READ | GENERIC_WRITE | WRITE_DAC;
const PRIVATE_FILE_READ_ACCESS: u32 = GENERIC_READ | READ_CONTROL | WRITE_DAC;

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    if let Err(error) = apply_to_path(path, PrivatePathKind::Directory) {
        let _ = fs::remove_dir(path);
        return Err(error);
    }
    Ok(())
}

fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .access_mode(PRIVATE_FILE_ACCESS);
    options
}

fn private_file_read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).access_mode(PRIVATE_FILE_READ_ACCESS);
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

pub(super) fn open_private_read(path: &Path) -> io::Result<(File, PrivateFileReadStatus)> {
    let file = private_file_read_options().open(path)?;
    let already_private = verify_handle(&file, PrivatePathKind::File).is_ok();
    super::finish_private_read(file, already_private, |file| {
        apply_to_handle(file, PrivatePathKind::File)
    })
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

fn descriptor_dacl(descriptor: &SecurityDescriptor) -> io::Result<&Acl> {
    let dacl = descriptor
        .dacl()
        .ok_or_else(|| postcondition_failed("security descriptor has no DACL"))?;
    if dacl.len() != 2 {
        return Err(postcondition_failed(
            "DACL does not contain exactly two entries",
        ));
    }

    Ok(dacl)
}

fn expected_ace_flags(kind: PrivatePathKind) -> AceFlags {
    match kind {
        PrivatePathKind::File => AceFlags::empty(),
        PrivatePathKind::Directory => AceFlags::ContainerInherit | AceFlags::ObjectInherit,
    }
}

fn expected_principal_sids() -> io::Result<(LocalBox<Sid>, LocalBox<Sid>)> {
    let user_sid = windows_permissions::utilities::current_process_sid()
        .map_err(|_| postcondition_failed("could not resolve the current process SID"))?;
    let system_sid: LocalBox<Sid> = "S-1-5-18"
        .parse()
        .map_err(|_| postcondition_failed("could not resolve the SYSTEM SID"))?;
    Ok((user_sid, system_sid))
}

fn validate_ace(dacl: &Acl, index: u32, expected_flags: AceFlags) -> io::Result<&Sid> {
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

    ace.sid()
        .ok_or_else(|| postcondition_failed("DACL entry has no principal"))
}

fn classify_principal(
    sid: &Sid,
    user_sid: &Sid,
    system_sid: &Sid,
    saw_user: &mut bool,
    saw_system: &mut bool,
) -> io::Result<()> {
    if sid == user_sid {
        if *saw_user {
            return Err(postcondition_failed("DACL contains a duplicate user entry"));
        }
        *saw_user = true;
    } else if sid == system_sid {
        if *saw_system {
            return Err(postcondition_failed(
                "DACL contains a duplicate SYSTEM entry",
            ));
        }
        *saw_system = true;
    } else {
        return Err(postcondition_failed(
            "DACL contains a principal outside the private-filesystem contract",
        ));
    }
    Ok(())
}

fn verify_required_principals(saw_user: bool, saw_system: bool) -> io::Result<()> {
    if !saw_user || !saw_system {
        return Err(postcondition_failed(
            "DACL is missing the current user or SYSTEM entry",
        ));
    }
    Ok(())
}

fn verify_descriptor(descriptor: &SecurityDescriptor, kind: PrivatePathKind) -> io::Result<()> {
    verify_dacl_sddl(descriptor, kind)?;

    let dacl = descriptor_dacl(descriptor)?;
    let (user_sid, system_sid) = expected_principal_sids()?;
    let expected_flags = expected_ace_flags(kind);

    let mut saw_user = false;
    let mut saw_system = false;
    for index in 0..dacl.len() {
        let sid = validate_ace(dacl, index, expected_flags)?;
        classify_principal(sid, &user_sid, &system_sid, &mut saw_user, &mut saw_system)?;
    }

    verify_required_principals(saw_user, saw_system)
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
    use super::{
        PrivatePathKind, descriptor_dacl, parse_dacl_header, validate_ace, verify_descriptor,
        verify_required_principals,
    };
    use std::io::{self, ErrorKind};
    use windows_permissions::{LocalBox, SecurityDescriptor};

    const ERROR_PREFIX: &str = "private filesystem DACL verification failed: ";

    fn descriptor_from_body(body: &str) -> LocalBox<SecurityDescriptor> {
        format!("D:P{body}")
            .parse()
            .expect("test security descriptor should parse")
    }

    fn current_user_sid() -> String {
        windows_permissions::utilities::current_process_sid()
            .expect("current process SID should be available")
            .to_string()
    }

    fn ace(ace_type: &str, flags: &str, rights: &str, sid: &str) -> String {
        format!("({ace_type};{flags};{rights};;;{sid})")
    }

    fn valid_body(kind: PrivatePathKind) -> String {
        let flags = match kind {
            PrivatePathKind::File => "",
            PrivatePathKind::Directory => "OICI",
        };
        let user_sid = current_user_sid();
        format!(
            "{}{}",
            ace("A", flags, "FA", &user_sid),
            ace("A", flags, "FA", "SY")
        )
    }

    fn assert_error(error: &io::Error, reason: &str) {
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), format!("{ERROR_PREFIX}{reason}"));
    }

    #[test]
    fn verifies_a_valid_file_descriptor() {
        let descriptor = descriptor_from_body(&valid_body(PrivatePathKind::File));

        verify_descriptor(&descriptor, PrivatePathKind::File)
            .expect("valid file descriptor should verify");
    }

    #[test]
    fn verifies_a_valid_directory_descriptor_with_object_and_container_inheritance() {
        let descriptor = descriptor_from_body(&valid_body(PrivatePathKind::Directory));

        verify_descriptor(&descriptor, PrivatePathKind::Directory)
            .expect("valid directory descriptor should verify");
    }

    #[test]
    fn rejects_an_unexpected_ace_type_or_permissions() {
        let user_sid = current_user_sid();
        for (ace_type, rights) in [("D", "FA"), ("A", "FR")] {
            let body = format!(
                "{}{}",
                ace(ace_type, "", rights, &user_sid),
                ace("A", "", "FA", "SY")
            );
            let descriptor = descriptor_from_body(&body);
            let error = verify_descriptor(&descriptor, PrivatePathKind::File)
                .expect_err("unexpected ACE metadata should be rejected");

            assert_error(
                &error,
                "DACL contains unexpected ACE control or inheritance flags",
            );
        }
    }

    #[test]
    fn rejects_unexpected_ace_inheritance_flags() {
        let user_sid = current_user_sid();
        for (kind, flags) in [
            (PrivatePathKind::File, "OICI"),
            (PrivatePathKind::Directory, ""),
        ] {
            let body = format!(
                "{}{}",
                ace("A", flags, "FA", &user_sid),
                ace("A", flags, "FA", "SY")
            );
            let descriptor = descriptor_from_body(&body);
            let error = verify_descriptor(&descriptor, kind)
                .expect_err("unexpected ACE inheritance should be rejected");

            assert_error(
                &error,
                "DACL contains unexpected ACE control or inheritance flags",
            );
        }
    }

    #[test]
    fn rejects_a_principal_additional_to_the_private_contract() {
        let body = format!(
            "{}{}",
            valid_body(PrivatePathKind::File),
            ace("A", "", "FA", "WD")
        );
        let descriptor = descriptor_from_body(&body);
        let error = verify_descriptor(&descriptor, PrivatePathKind::File)
            .expect_err("an additional principal should be rejected");

        assert_error(&error, "DACL does not contain exactly two ACE descriptors");
    }

    #[test]
    fn rejects_a_duplicate_user_or_system_entry() {
        let user_sid = current_user_sid();
        for (body, reason) in [
            (
                format!(
                    "{}{}",
                    ace("A", "", "FA", &user_sid),
                    ace("A", "", "FA", &user_sid)
                ),
                "DACL contains a duplicate user entry",
            ),
            (
                format!("{}{}", ace("A", "", "FA", "SY"), ace("A", "", "FA", "SY")),
                "DACL contains a duplicate SYSTEM entry",
            ),
        ] {
            let descriptor = descriptor_from_body(&body);
            let error = verify_descriptor(&descriptor, PrivatePathKind::File)
                .expect_err("duplicate principals should be rejected");

            assert_error(&error, reason);
        }
    }

    #[test]
    fn reports_missing_principals_with_the_exact_error() {
        for (saw_user, saw_system) in [(false, true), (true, false), (false, false)] {
            let error = verify_required_principals(saw_user, saw_system)
                .expect_err("a missing required principal should be rejected");
            assert_error(&error, "DACL is missing the current user or SYSTEM entry");
        }
    }

    #[test]
    fn reports_a_missing_dacl_with_the_exact_error() {
        let descriptor: LocalBox<SecurityDescriptor> = "O:S-1-5-18"
            .parse()
            .expect("test descriptor without a DACL should parse");
        let error = descriptor_dacl(&descriptor).expect_err("a missing DACL should be rejected");

        assert_error(&error, "security descriptor has no DACL");
    }

    #[test]
    fn reports_an_unexpected_dacl_entry_count_with_the_exact_error() {
        let descriptor = descriptor_from_body(&ace("A", "", "FA", "SY"));
        let error =
            descriptor_dacl(&descriptor).expect_err("a DACL with one entry should be rejected");

        assert_error(&error, "DACL does not contain exactly two entries");
    }

    #[test]
    fn reports_an_unreadable_dacl_entry_with_the_exact_error() {
        let descriptor = descriptor_from_body(&valid_body(PrivatePathKind::File));
        let dacl = descriptor
            .dacl()
            .expect("test descriptor should have a DACL");
        let error = validate_ace(dacl, dacl.len(), super::AceFlags::empty())
            .expect_err("an out-of-range DACL entry should be rejected");

        assert_error(&error, "DACL entry could not be read");
    }

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
