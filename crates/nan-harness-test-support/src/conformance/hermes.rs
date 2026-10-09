use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(super) fn configure(
    mut command: crate::terminal::TerminalCommand,
    home: &Path,
) -> io::Result<crate::terminal::TerminalCommand> {
    command = command.env("HERMES_HOME", prepare_home(home)?);
    for (name, value) in super::constants::HERMES_OPTIONAL_CREDENTIALS_CLEARED {
        command = command.env(*name, *value);
    }
    Ok(command)
}

/// Borrow installed dependencies without importing credentials or user configuration.
/// Hermes PM resolves committed environments beneath the active data home, even
/// when the executable itself belongs to a different, already installed home.
pub(super) fn prepare_home(home: &Path) -> io::Result<PathBuf> {
    let configured = std::env::var_os("HERMES_HOME").filter(|value| !value.is_empty());
    #[cfg(windows)]
    let source = configured.map(PathBuf::from).or_else(|| {
        std::env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("hermes"))
    });
    #[cfg(not(windows))]
    let source = configured
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|root| PathBuf::from(root).join(".hermes")));
    let target = home.join(".hermes");
    nan_harness_private_fs::create_private_dir_all(&target)?;
    if let Some(source) = source {
        link_dependencies(&source, &target)?;
    }
    Ok(target)
}

fn link_dependencies(source: &Path, target: &Path) -> io::Result<()> {
    for name in ["installs", "tools"] {
        let path = source.join(name);
        if !path.try_exists()? {
            continue;
        }
        let path = fs::canonicalize(path)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&path, target.join(name))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&path, target.join(name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_home_borrows_only_dependency_state_and_preserves_its_owner() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        for name in ["installs", "tools", "plugins", "sessions"] {
            fs::create_dir(source.path().join(name)).unwrap();
            fs::write(source.path().join(name).join("fixture"), name).unwrap();
        }
        fs::write(source.path().join(".env"), "synthetic-secret").unwrap();
        fs::write(source.path().join("config.yaml"), "synthetic-settings").unwrap();
        link_dependencies(source.path(), target.path()).unwrap();
        for name in ["installs", "tools"] {
            assert_eq!(
                fs::canonicalize(target.path().join(name)).unwrap(),
                fs::canonicalize(source.path().join(name)).unwrap()
            );
        }
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 2);
        target.close().unwrap();
        assert_eq!(
            fs::read_to_string(source.path().join("installs/fixture")).unwrap(),
            "installs"
        );
        assert_eq!(
            fs::read_to_string(source.path().join(".env")).unwrap(),
            "synthetic-secret"
        );
    }

    #[test]
    fn legacy_installations_need_no_dependency_links() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        link_dependencies(source.path(), target.path()).unwrap();
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    }
}
