//! Explicit source-pinned Chat configuration for a disposable Linux profile.
use super::{ClaudeDesktopError, DesktopPaths};
use std::path::{Path, PathBuf};

pub(super) fn requested(paths: &DesktopPaths) -> Result<bool, ClaudeDesktopError> {
    let value = match std::env::var("NANH_CLAUDE_LINUX_CHAT_ONLY") {
        Err(std::env::VarError::NotPresent) => return Ok(false),
        Ok(value) => value,
        Err(std::env::VarError::NotUnicode(_)) => return Err(ClaudeDesktopError::InvalidStatePath),
    };
    let workspace = std::env::current_dir().map_err(|_| ClaudeDesktopError::InvalidStatePath)?;
    let policy = value == "1"
        && cfg!(target_os = "linux")
        && std::env::var("RUNNER_OS").as_deref() == Ok("Linux")
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline")
        && std::env::var("NANH_CLAUDE_LINUX_SOURCE_POLICY").as_deref() == Ok("official-2.9939.4")
        && std::env::var_os("CLAUDE_USER_DATA_DIR").is_none()
        && std::env::var_os("CLAUDE_CDP_AUTH").is_none();
    let bound = [
        ("HOME", workspace.join("profile/home")),
        ("XDG_CONFIG_HOME", workspace.join("profile/config")),
        ("NAN_HARNESS_CONFIG_DIR", workspace.join("profile/nanh")),
    ]
    .iter()
    .all(|(key, expected)| std::env::var_os(key).map(PathBuf::from).as_ref() == Some(expected));
    let facts = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from);
    if !policy
        || !bound
        || facts.as_deref().is_none_or(|p| !private_canonical(p))
        || !profile_bound(paths, &workspace)
    {
        return Err(ClaudeDesktopError::InvalidStatePath);
    }
    Ok(true)
}

fn private_canonical(path: &Path) -> bool {
    super::qualification_config::private_directory(path)
        && path.canonicalize().is_ok_and(|canonical| canonical == path)
}

fn profile_bound(paths: &DesktopPaths, workspace: &Path) -> bool {
    let profile = workspace.join("profile");
    if !workspace.is_absolute()
        || !workspace.canonicalize().is_ok_and(|p| p == workspace)
        || [
            profile.clone(),
            profile.join("home"),
            profile.join("config"),
            profile.join("nanh"),
        ]
        .iter()
        .any(|p| !private_canonical(p))
    {
        return false;
    }
    let expected = DesktopPaths::new(
        &profile.join("config/Claude"),
        &profile.join("config/Claude-3p"),
        &profile.join("nanh"),
    );
    if paths.documents() != expected.documents()
        || paths.receipt != expected.receipt
        || paths.backup_directory != expected.backup_directory
        || paths.lock != expected.lock
    {
        return false;
    }
    // Missing configuration is legitimate before apply_gateway; existing components
    // must never redirect the fixed private root through a symlink.
    paths
        .documents()
        .into_iter()
        .chain([
            paths.receipt.as_path(),
            paths.backup_directory.as_path(),
            paths.lock.as_path(),
        ])
        .all(|p| {
            p.strip_prefix(&profile).is_ok_and(|suffix| {
                let mut current = profile.clone();
                suffix.components().all(|component| {
                    current.push(component);
                    match std::fs::symlink_metadata(&current) {
                        Ok(metadata) => !metadata.file_type().is_symlink(),
                        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
                    }
                })
            })
        })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn fixed_roots_reject_redirects_and_preserve_unrelated_state() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        for part in ["profile", "profile/home", "profile/config", "profile/nanh"] {
            let p = root.join(part);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let paths = DesktopPaths::new(
            &root.join("profile/config/Claude"),
            &root.join("profile/config/Claude-3p"),
            &root.join("profile/nanh"),
        );
        let sentinel = root.join("profile/nanh/unrelated");
        std::fs::write(&sentinel, b"private sentinel").unwrap();
        assert!(profile_bound(&paths, &root));
        let wrong = DesktopPaths::new(
            &root.join("profile/home/.config/Claude"),
            &root.join("profile/home/.config/Claude-3p"),
            &root.join("profile/nanh"),
        );
        assert!(!profile_bound(&wrong, &root));
        symlink(
            root.join("profile/home"),
            root.join("profile/config/Claude"),
        )
        .unwrap();
        assert!(!profile_bound(&paths, &root));
        std::fs::remove_file(root.join("profile/config/Claude")).unwrap();
        std::fs::set_permissions(
            root.join("profile/config"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(!profile_bound(&paths, &root));
        assert_eq!(std::fs::read(sentinel).unwrap(), b"private sentinel");
        std::fs::remove_dir_all(root).unwrap();
    }
}
