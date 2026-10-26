use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

/// Native SQLite turns a verbatim Windows path into a file://%3F/ URI. Export
/// ordinary spelling only after proving it still names the canonical directory;
/// retain the canonical path separately for private filesystem operations.
pub(super) fn for_sdk(canonical: &Path) -> io::Result<PathBuf> {
    let native = ordinary_spelling(canonical)?;
    if fs::canonicalize(&native)? != canonical {
        return Err(invalid_path());
    }
    Ok(native)
}

fn ordinary_spelling(canonical: &Path) -> io::Result<PathBuf> {
    let mut components = canonical.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(invalid_path());
    };
    let mut native = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut root = OsString::from("\\\\");
            root.push(server);
            root.push("\\");
            root.push(share);
            root.push("\\");
            PathBuf::from(root)
        }
        Prefix::Disk(_) | Prefix::UNC(..) => return Ok(canonical.to_owned()),
        _ => return Err(invalid_path()),
    };
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => native.push(name),
            _ => return Err(invalid_path()),
        }
    }
    Ok(native)
}

fn invalid_path() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "NH-HERMES-PROFILE-PATH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_home_keeps_physical_identity_without_a_verbatim_uri_authority() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("NaN profile 日本語");
        fs::create_dir(&profile).unwrap();
        let canonical = fs::canonicalize(&profile).unwrap();
        let native = for_sdk(&canonical).unwrap();
        assert_eq!(fs::canonicalize(&native).unwrap(), canonical);
        assert!(matches!(
            native.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
        ));
    }

    #[test]
    fn unc_spelling_preserves_server_share_and_components_without_network_access() {
        let native = ordinary_spelling(Path::new(r"\\?\UNC\server\share\NaN profile")).unwrap();
        assert_eq!(native, Path::new(r"\\server\share\NaN profile"));
        assert!(ordinary_spelling(Path::new(r"\\.\pipe\native")).is_err());
        assert!(ordinary_spelling(Path::new(r"\\?\Volume{native}\profile")).is_err());
    }
}
