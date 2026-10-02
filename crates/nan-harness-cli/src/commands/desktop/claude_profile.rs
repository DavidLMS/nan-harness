//! Frozen hosted trials of Claude's isolated and default-native storage.
use std::{io, path::Path};

fn rejected() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Claude profile trial rejected",
    )
}

pub(super) fn arguments(executable: Option<&Path>) -> io::Result<Vec<String>> {
    let Some(policy) = std::env::var_os("NANH_CLAUDE_MAC_PROFILE_POLICY") else {
        return Ok(Vec::new());
    };
    if (policy != "electron-user-data-dir" && policy != "native-known-folders")
        || !cfg!(target_os = "macos")
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("macOS")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        || std::env::var_os("CLAUDE_USER_DATA_DIR").is_some()
        || std::env::var_os("CLAUDE_CDP_AUTH").is_some()
    {
        return Err(rejected());
    }
    let executable = executable.ok_or_else(rejected)?;
    let workspace = std::env::current_dir()?;
    let home = std::env::var_os("HOME").ok_or_else(rejected)?;
    let facts = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").ok_or_else(rejected)?;
    let root = validated_root(
        &workspace,
        Path::new(&home),
        Path::new(&facts),
        executable,
        policy == "native-known-folders",
    )?;
    if policy == "native-known-folders" {
        return Ok(Vec::new());
    }
    // Claude's frozen bootstrap derives its third-party root by appending -3p.
    Ok(vec![format!(
        "--user-data-dir={}",
        root.to_str().ok_or_else(rejected)?
    )])
}

fn private_directory(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_dir() || path.canonicalize().ok().as_deref() != Some(path) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode().trailing_zeros() >= 6
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn regular(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
        && path.canonicalize().ok().as_deref() == Some(path)
}

fn validated_root(
    workspace: &Path,
    home: &Path,
    facts: &Path,
    executable: &Path,
    native: bool,
) -> io::Result<std::path::PathBuf> {
    let profile = workspace.join("profile");
    if (!native && home != profile.join("home"))
        || !private_directory(&profile)
        || (!native && !private_directory(home))
        || (native && home.canonicalize().ok().as_deref() != Some(home))
        || !private_directory(facts)
        || !regular(executable)
    {
        return Err(rejected());
    }
    let contents = executable
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(Path::parent)
        .filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
        .ok_or_else(rejected)?;
    if executable.file_name().is_none_or(|n| n != "Claude")
        || contents
            .parent()
            .is_none_or(|p| p.file_name().is_none_or(|n| n != "Claude.app"))
        || !regular(&contents.join("Info.plist"))
        || !regular(&contents.join("Resources/app.asar"))
    {
        return Err(rejected());
    }
    let support = home.join("Library/Application Support");
    let normal = support.join("Claude");
    for root in [&normal, &support.join("Claude-3p")] {
        if !private_directory(root) || !regular(&root.join("claude_desktop_config.json")) {
            return Err(rejected());
        }
    }
    Ok(normal)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    #[test]
    fn requires_private_generated_pair_and_direct_canonical_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().canonicalize().unwrap();
        let home = workspace.join("profile/home");
        let facts = workspace.join("facts");
        let contents = workspace.join("Claude.app/Contents");
        let executable = contents.join("MacOS/Claude");
        for directory in [
            &home,
            &facts,
            &contents.join("MacOS"),
            &contents.join("Resources"),
        ] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::set_permissions(
            workspace.join("profile"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&facts, std::fs::Permissions::from_mode(0o700)).unwrap();
        for path in [
            &executable,
            &contents.join("Info.plist"),
            &contents.join("Resources/app.asar"),
        ] {
            std::fs::write(path, b"synthetic").unwrap();
        }
        for name in ["Claude", "Claude-3p"] {
            let root = home.join("Library/Application Support").join(name);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(root.join("claude_desktop_config.json"), b"{}").unwrap();
        }
        assert_eq!(
            validated_root(&workspace, &home, &facts, &executable, false).unwrap(),
            home.join("Library/Application Support/Claude")
        );
        assert!(validated_root(&workspace, temp.path(), &facts, &executable, false).is_err());
        let native_home = workspace.join("native-home");
        for name in ["Claude", "Claude-3p"] {
            let root = native_home.join("Library/Application Support").join(name);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(root.join("claude_desktop_config.json"), b"{}").unwrap();
        }
        assert!(validated_root(&workspace, &native_home, &facts, &executable, false).is_err());
        assert_eq!(
            validated_root(&workspace, &native_home, &facts, &executable, true).unwrap(),
            native_home.join("Library/Application Support/Claude")
        );
        std::fs::set_permissions(
            native_home.join("Library/Application Support/Claude"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(validated_root(&workspace, &native_home, &facts, &executable, true).is_err());
        let config = home.join("Library/Application Support/Claude-3p/claude_desktop_config.json");
        std::fs::remove_file(&config).unwrap();
        assert!(validated_root(&workspace, &home, &facts, &executable, false).is_err());
        symlink(contents.join("Info.plist"), config).unwrap();
        assert!(validated_root(&workspace, &home, &facts, &executable, false).is_err());
    }
}
