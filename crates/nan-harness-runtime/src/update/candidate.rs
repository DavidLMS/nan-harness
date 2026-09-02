use super::UpdateError;
use semver::Version;
use sha2::{Digest as _, Sha256};
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::fs;

pub(super) struct CandidateDigest(Sha256);

impl CandidateDigest {
    pub(super) fn new() -> Self {
        Self(Sha256::new())
    }

    pub(super) fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    pub(super) fn matches(self, expected: &str) -> bool {
        let actual = hex_digest(self.0.finalize());
        constant_time_hex_eq(&actual, expected)
    }
}

pub(super) fn verify_candidate(candidate: &Path, version: &Version) -> Result<(), UpdateError> {
    let output = Command::new(candidate)
        .arg("--version")
        .output()
        .map_err(UpdateError::ExecuteCandidate)?;
    if !output.status.success() {
        return Err(UpdateError::CandidateRejected);
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let expected = version.to_string();
    if !text.split_whitespace().any(|part| part == expected) {
        return Err(UpdateError::CandidateVersionMismatch {
            expected: version.clone(),
            output: bounded_output(&text),
        });
    }
    Ok(())
}

pub(super) fn validate_sha256(value: &str) -> Result<(), UpdateError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(UpdateError::InvalidChecksum)
    }
}

fn constant_time_hex_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left.to_ascii_lowercase() ^ right.to_ascii_lowercase())
        })
        == 0
}

pub(super) fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(unix)]
pub(super) fn make_executable(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(UpdateError::SetCandidatePermissions)
}

#[cfg(windows)]
pub(super) fn make_executable(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

fn bounded_output(value: &str) -> String {
    value.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::{CandidateDigest, constant_time_hex_eq, hex_digest};
    use sha2::{Digest as _, Sha256};

    #[test]
    fn checksum_comparison_accepts_hex_case_without_skipping_bytes() {
        let checksum = hex_digest(Sha256::digest([0xab; 32]));
        let mut digest = CandidateDigest::new();
        digest.update(&[0xab; 32]);

        assert!(constant_time_hex_eq(&checksum, &checksum.to_uppercase()));
        assert!(digest.matches(&checksum));
        assert!(!constant_time_hex_eq(&checksum, &"a".repeat(63)));
    }
}
