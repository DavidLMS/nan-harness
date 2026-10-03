//! Validate an existing private facts directory before resolving its native path.

use std::path::{Path, PathBuf};

pub(super) fn canonical_directory(directory: &Path) -> Option<PathBuf> {
    if !directory.is_absolute() {
        return None;
    }
    let metadata = std::fs::symlink_metadata(directory).ok()?;
    if !metadata.is_dir() || metadata.is_symlink() {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        // Canonicalization adds a verbatim prefix on Windows. Validate every
        // original component first, so resolving that prefix cannot conceal a
        // junction or another reparse point in the supplied facts path.
        let mut ancestor = PathBuf::new();
        let mut rooted = false;
        for component in directory.components() {
            ancestor.push(component.as_os_str());
            // Verbatim disk prefixes are considered absolute before RootDir,
            // but the prefix alone is not the drive-root directory to inspect.
            rooted |= matches!(component, std::path::Component::RootDir);
            if !rooted {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&ancestor).ok()?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return None;
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let canonical = directory.canonicalize().ok()?;
    #[cfg(not(windows))]
    if canonical != directory {
        return None;
    }
    Some(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_absolute_private_directories_are_admitted() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let facts = root.join("facts");
        nan_harness_private_fs::create_private_dir(&facts).unwrap();
        #[cfg(windows)]
        assert!(
            canonical_directory(&facts) == Some(facts.canonicalize().unwrap()),
            "fixture ancestor metadata: {:?}",
            ancestor_trace(&facts)
        );
        #[cfg(not(windows))]
        assert_eq!(
            canonical_directory(&facts),
            Some(facts.canonicalize().unwrap())
        );
        assert!(canonical_directory(Path::new("facts")).is_none());
        assert!(canonical_directory(&root.join("missing")).is_none());
        let file = root.join("file");
        std::fs::write(&file, b"synthetic").unwrap();
        assert!(canonical_directory(&file).is_none());
    }

    // Synthetic fixture diagnostics contain only ordinal and metadata booleans.
    #[cfg(windows)]
    fn ancestor_trace(path: &Path) -> Vec<(usize, bool, bool, bool)> {
        use std::os::windows::fs::MetadataExt as _;
        let mut ancestor = PathBuf::new();
        let mut rooted = false;
        let mut trace = Vec::new();
        for (ordinal, component) in path.components().enumerate() {
            ancestor.push(component.as_os_str());
            rooted |= matches!(component, std::path::Component::RootDir);
            if !rooted {
                continue;
            }
            match std::fs::symlink_metadata(&ancestor) {
                Ok(metadata) => trace.push((
                    ordinal,
                    metadata.is_dir(),
                    metadata.file_attributes() & 0x400 != 0,
                    false,
                )),
                Err(_) => trace.push((ordinal, false, false, true)),
            }
        }
        trace
    }

    #[cfg(windows)]
    #[test]
    fn normal_and_verbatim_spellings_have_the_same_validated_directory() {
        let temp = tempfile::tempdir().unwrap();
        let facts = temp.path().join("facts");
        nan_harness_private_fs::create_private_dir(&facts).unwrap();
        let canonical = facts.canonicalize().unwrap();
        let normal = PathBuf::from(canonical.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
        assert!(
            canonical_directory(&normal) == Some(canonical.clone()),
            "normal ancestor metadata: {:?}",
            ancestor_trace(&normal)
        );
        assert!(
            canonical_directory(&canonical) == Some(canonical),
            "verbatim ancestor metadata: {:?}",
            ancestor_trace(&facts.canonicalize().unwrap())
        );
    }

    #[cfg(windows)]
    #[test]
    fn reparse_directory_and_ancestor_are_rejected_before_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("target");
        nan_harness_private_fs::create_private_dir(&target).unwrap();
        nan_harness_private_fs::create_private_dir(&target.join("facts")).unwrap();
        let link = root.join("link");
        // Junctions do not require the symbolic-link privilege on hosted Windows.
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(canonical_directory(&link).is_none());
        assert!(canonical_directory(&link.join("facts")).is_none());
        // Remove only the junction before TempDir recursively removes its tree.
        std::fs::remove_dir(&link).unwrap();
        assert!(target.join("facts").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_and_canonical_spelling_remain_strict() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let facts = root.join("facts");
        nan_harness_private_fs::create_private_dir(&facts).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&facts, &link).unwrap();
        assert!(canonical_directory(&link).is_none());
        std::fs::set_permissions(&facts, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(canonical_directory(&facts).is_none());
    }
}
