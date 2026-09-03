use super::{FailureClass, FailureIdentity};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;

pub(super) fn failure_fingerprint(
    identity: &FailureIdentity<'_>,
    class: FailureClass,
    phase: &str,
    code: Option<&str>,
) -> String {
    let source = format!(
        "{}|{}|{}|{}|{}|{}|{:?}|{}|{}",
        identity.harness,
        identity.harness_version,
        identity.operating_system,
        identity.architecture,
        identity.tier.as_str(),
        identity.scenario,
        class,
        phase,
        code.unwrap_or_default()
    );
    sha256_hex(source.as_bytes())
}

pub(crate) fn sha256_hex(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}
