//! Resolve the publisher's package index without modifying host APT configuration.

use super::{InstallError, download_file, install_owned};
use crate::{
    catalog::{Installation, PackageFormat},
    journal::Journal,
};
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::collections::BTreeMap;

pub(super) async fn install(
    kind: DesktopHarnessKind,
    journal: &mut Journal,
    base_url: &str,
    architecture: &str,
) -> Result<Installation, InstallError> {
    let name = format!("install-{kind}");
    let root = journal.reserve(&name)?;
    let result = async {
        let index = root.join("Packages");
        download_file(
            &format!("{base_url}dists/stable/main/binary-{architecture}/Packages"),
            &index,
            1024 * 1024,
        )
        .await?;
        let text = std::fs::read_to_string(index)?;
        let (filename, sha256) = newest_package(&text, architecture)?;
        install_owned(
            kind,
            &format!("{base_url}{filename}"),
            PackageFormat::Deb,
            &root,
            Some(&sha256),
        )
        .await
    }
    .await;
    journal.seal(&name)?;
    result
}

fn newest_package(text: &str, architecture: &str) -> Result<(String, String), InstallError> {
    let mut candidates = BTreeMap::new();
    for paragraph in text
        .split("\n\n")
        .filter(|paragraph| !paragraph.trim().is_empty())
    {
        let mut fields = BTreeMap::new();
        for line in paragraph.lines().filter(|line| !line.starts_with(' ')) {
            let (name, value) = line.split_once(": ").ok_or(InstallError::Archive)?;
            if fields.insert(name, value).is_some() {
                return Err(InstallError::Archive);
            }
        }
        if fields.get("Package") != Some(&"claude-desktop")
            || fields.get("Architecture") != Some(&architecture)
        {
            continue;
        }
        let version = Version::parse(fields.get("Version").ok_or(InstallError::Archive)?)
            .map_err(|_| InstallError::Archive)?;
        let filename =
            format!("pool/main/c/claude-desktop/claude-desktop_{version}_{architecture}.deb");
        let sha256 = fields.get("SHA256").ok_or(InstallError::Archive)?;
        if fields.get("Filename") != Some(&filename.as_str())
            || sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || candidates
                .insert(version, (filename, sha256.to_ascii_lowercase()))
                .is_some()
        {
            return Err(InstallError::Archive);
        }
    }
    candidates
        .pop_last()
        .map(|(_, package)| package)
        .ok_or(InstallError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str, architecture: &str) -> String {
        format!(
            "Package: claude-desktop\nVersion: {version}\nArchitecture: {architecture}\nFilename: pool/main/c/claude-desktop/claude-desktop_{version}_{architecture}.deb\nSHA256: {}\n",
            "a".repeat(64)
        )
    }

    #[test]
    fn selects_newest_architecture_and_binds_its_checksum() {
        let index = [
            package("1.9.0", "arm64"),
            package("1.10.0", "arm64"),
            package("2.0.0", "amd64"),
        ]
        .join("\n");
        let (filename, digest) = newest_package(&index, "arm64").unwrap();
        assert!(filename.ends_with("_1.10.0_arm64.deb"));
        assert_eq!(digest, "a".repeat(64));
    }

    #[test]
    fn rejects_duplicate_identity_wrong_checksum_and_untrusted_path() {
        let valid = package("1.0.0", "amd64");
        for invalid in [
            format!("{valid}\n{valid}"),
            valid.replace(&"a".repeat(64), "bad"),
            valid.replace("pool/main/c/", "../"),
        ] {
            assert!(newest_package(&invalid, "amd64").is_err());
        }
    }
}
