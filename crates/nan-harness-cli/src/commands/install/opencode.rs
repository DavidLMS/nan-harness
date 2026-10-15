use super::{InstallError, installer::run_command};
use nan_harness_core::HarnessKind;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

const RECEIPT: &str = ".nanh-opencode-installation";

struct InstallationLock(fs::File);

impl Drop for InstallationLock {
    fn drop(&mut self) {
        // A concurrent fork can retain the open file description until exec.
        // Closing our descriptor alone would keep that inherited lock alive.
        let _ = self.0.unlock();
    }
}

fn root() -> Option<PathBuf> {
    super::super::persistence::config_directory().map(|path| path.join("installations/opencode"))
}

fn binary(prefix: &Path) -> PathBuf {
    prefix.join("node_modules/.bin").join(if cfg!(windows) {
        "opencode.cmd"
    } else {
        "opencode"
    })
}

pub(crate) fn installed(major: u8) -> Option<PathBuf> {
    let prefix = root()?.join(major.to_string());
    (fs::read_to_string(prefix.join(RECEIPT)).ok()? == major.to_string()).then(|| binary(&prefix))
}

pub(crate) fn offer(major: u8) -> Result<Option<PathBuf>, InstallError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(prepare(io::Error::other(
            "OpenCode is not installed; run interactively or pass --executable PATH",
        )));
    }
    super::check_required_runtime(HarnessKind::OpenCode)?;
    let npm =
        run_command(OsStr::new("npm"), &["--version"], Command::output).map_err(|source| {
            InstallError::CommandStart {
                harness: HarnessKind::OpenCode,
                program: "npm",
                source,
            }
        })?;
    if !npm.status.success() {
        return Err(InstallError::CommandFailed {
            harness: HarnessKind::OpenCode,
            program: "npm",
            exit_code: npm.status.code(),
        });
    }
    let name = format!("OpenCode {major}");
    eprintln!(
        "{}",
        nan_harness_i18n::messages::install_was_not_found_install_the_latest_official_release_now(
            nan_harness_i18n::locale(),
            &name
        )
    );
    eprintln!(
        "{}",
        nan_harness_i18n::messages::opencode_selection_install_location(nan_harness_i18n::locale())
    );
    eprint!(
        "{}",
        nan_harness_i18n::messages::install_install_y_n(nan_harness_i18n::locale(), &name)
    );
    io::stderr().flush().map_err(InstallError::Prompt)?;
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(InstallError::Prompt)?;
    if !super::is_affirmative(&response) {
        return Ok(None);
    }
    let root =
        root().ok_or_else(|| prepare(io::Error::other("missing configuration directory")))?;
    install_into(&root, major).map(Some)
}

fn install_into(root: &Path, major: u8) -> Result<PathBuf, InstallError> {
    install_with(root, major, |prefix| install_package(prefix, major))
}

fn install_with(
    root: &Path,
    major: u8,
    install: impl FnOnce(&Path) -> Result<(), InstallError>,
) -> Result<PathBuf, InstallError> {
    reject_symlink(root)?;
    fs::create_dir_all(root).map_err(prepare)?;
    let lock_path = root.join("install.lock");
    reject_symlink(&lock_path)?;
    let lock = nan_harness_private_fs::open_private_truncate(&lock_path).map_err(prepare)?;
    lock.try_lock()
        .map_err(|error| prepare(io::Error::other(error)))?;
    let _lock = InstallationLock(lock);
    let destination = root.join(major.to_string());
    reject_symlink(&destination)?;
    if destination.exists() {
        if fs::read_to_string(destination.join(RECEIPT)).map_err(prepare)? != major.to_string() {
            return Err(prepare(io::Error::other(
                "OpenCode installation ownership changed",
            )));
        }
        verify(&binary(&destination), major)?;
        return Ok(binary(&destination));
    }
    let temporary = tempfile::tempdir_in(root).map_err(prepare)?;
    install(temporary.path())?;
    verify(&binary(temporary.path()), major)?;
    fs::write(temporary.path().join(RECEIPT), major.to_string()).map_err(prepare)?;
    fs::rename(temporary.path(), &destination).map_err(prepare)?;
    Ok(binary(&destination))
}

fn install_package(directory: &Path, major: u8) -> Result<(), InstallError> {
    let prefix = directory
        .to_str()
        .ok_or_else(|| prepare(io::Error::other("non-UTF-8 installation path")))?;
    let package = if major == 1 {
        "opencode-ai@1"
    } else {
        "@opencode/cli@2"
    };
    let status = run_command(
        OsStr::new("npm"),
        &[
            "install",
            "--prefix",
            prefix,
            "--no-audit",
            "--no-fund",
            package,
        ],
        Command::status,
    )
    .map_err(|source| InstallError::CommandStart {
        harness: HarnessKind::OpenCode,
        program: "npm",
        source,
    })?;
    if !status.success() {
        return Err(InstallError::CommandFailed {
            harness: HarnessKind::OpenCode,
            program: "npm",
            exit_code: status.code(),
        });
    }
    Ok(())
}

fn verify(path: &Path, major: u8) -> Result<(), InstallError> {
    if super::super::opencode_selection::major_at(path).ok() == Some(major) {
        Ok(())
    } else {
        Err(prepare(io::Error::other(
            "installed OpenCode failed generation verification",
        )))
    }
}

fn reject_symlink(path: &Path) -> Result<(), InstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(prepare(io::Error::other(
            "refusing symbolic link installation path",
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(prepare(error)),
    }
}

fn prepare(source: io::Error) -> InstallError {
    InstallError::PrepareInstaller {
        harness: HarnessKind::OpenCode,
        source,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn fixture(prefix: &Path, major: u8) -> Result<(), InstallError> {
        let executable = binary(prefix);
        fs::create_dir_all(executable.parent().unwrap()).map_err(prepare)?;
        fs::write(
            &executable,
            format!("#!/bin/sh\necho 'opencode v{major}.0.27'\n"),
        )
        .map_err(prepare)?;
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).map_err(prepare)
    }

    #[test]
    fn completed_installation_releases_lock_with_an_inherited_descriptor() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("install.lock");
        let file = nan_harness_private_fs::open_private_truncate(&path).unwrap();
        file.try_lock().unwrap();
        let lock = InstallationLock(file);
        // A duplicate shares the lock's open file description, as fork does.
        let inherited = lock.0.try_clone().unwrap();
        let contender = nan_harness_private_fs::open_private_truncate(&path).unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(fs::TryLockError::WouldBlock)
        ));
        drop(lock);
        contender.try_lock().unwrap();
        contender.unlock().unwrap();
        drop(inherited);
    }

    #[test]
    fn staged_generations_coexist_and_owned_installation_is_reused() {
        let temporary = tempfile::tempdir().unwrap();
        let one = install_with(temporary.path(), 1, |prefix| fixture(prefix, 1)).unwrap();
        let two = install_with(temporary.path(), 2, |prefix| fixture(prefix, 2)).unwrap();
        assert!(one.exists() && two.exists());
        assert_ne!(one, two);
        assert_eq!(
            install_with(temporary.path(), 2, |_| panic!("must reuse")).unwrap(),
            two
        );
    }

    #[test]
    fn failed_or_wrong_generation_staging_is_never_published() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(install_with(temporary.path(), 2, |prefix| fixture(prefix, 1)).is_err());
        assert!(!temporary.path().join("2").exists());
        assert!(
            install_with(temporary.path(), 2, |_| Err(prepare(io::Error::other(
                "interrupted"
            ))))
            .is_err()
        );
        assert!(!temporary.path().join("2").exists());
        assert!(install_with(temporary.path(), 2, |prefix| fixture(prefix, 2)).is_ok());
    }

    #[test]
    fn unowned_destination_and_concurrent_installer_are_preserved() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("2");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("user-data"), b"keep").unwrap();
        assert!(install_with(temporary.path(), 2, |_| panic!("must not install")).is_err());
        assert_eq!(fs::read(destination.join("user-data")).unwrap(), b"keep");
        let lock =
            nan_harness_private_fs::open_private_truncate(&temporary.path().join("install.lock"))
                .unwrap();
        lock.lock().unwrap();
        assert!(install_with(temporary.path(), 1, |_| panic!("must not install")).is_err());
    }
}
