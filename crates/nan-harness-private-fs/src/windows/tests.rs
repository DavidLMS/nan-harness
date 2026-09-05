use super::{
    PrivatePathKind, descriptor_dacl, parse_dacl_header, validate_ace, verify_descriptor,
    verify_required_principals,
};
use std::io::{self, ErrorKind};
use windows_permissions::constants::AceFlags;
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
    let error = descriptor_dacl(&descriptor).expect_err("a DACL with one entry should be rejected");

    assert_error(&error, "DACL does not contain exactly two entries");
}

#[test]
fn reports_an_unreadable_dacl_entry_with_the_exact_error() {
    let descriptor = descriptor_from_body(&valid_body(PrivatePathKind::File));
    let dacl = descriptor
        .dacl()
        .expect("test descriptor should have a DACL");
    let error = validate_ace(dacl, dacl.len(), AceFlags::empty())
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
