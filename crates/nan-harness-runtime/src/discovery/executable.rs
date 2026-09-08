use super::DiscoveryError;
pub(super) use super::probe::run_command;
use nan_harness_core::{HarnessCompatibility, HarnessKind};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

/// Locates and validates a harness executable.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when an override is not executable or the harness cannot be found on
/// `PATH`.
pub fn locate_harness_executable(
    kind: HarnessKind,
    executable_override: Option<&Path>,
) -> Result<PathBuf, DiscoveryError> {
    match executable_override {
        Some(path) => validate_executable(path),
        None => find_executable(kind.binary_name())
            .ok_or_else(|| DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned())),
    }
}

pub(super) fn version_arguments(entry: &HarnessCompatibility) -> Result<Vec<&str>, DiscoveryError> {
    let mut parts = entry.command.split_ascii_whitespace();
    let executable = parts.next();
    let arguments = parts.collect::<Vec<_>>();
    if executable != Some(entry.id.binary_name()) || arguments.is_empty() {
        return Err(DiscoveryError::InvalidVersionCommand {
            harness: entry.id,
            command: entry.command.clone(),
        });
    }
    Ok(arguments)
}

fn validate_executable(path: &Path) -> Result<PathBuf, DiscoveryError> {
    if is_executable_file(path) {
        Ok(path.to_path_buf())
    } else {
        Err(DiscoveryError::InvalidExecutable(path.to_path_buf()))
    }
}

#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH");
    find_executable_in_path(binary_name, path.as_deref())
}

fn find_executable_in_path(binary_name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let path = path?;
    env::split_paths(path)
        .flat_map(|directory| executable_candidates(&directory, binary_name))
        .find(|candidate| is_executable_file(candidate))
}

fn executable_candidates(directory: &Path, binary_name: &str) -> Vec<PathBuf> {
    let base = directory.join(binary_name);
    if cfg!(windows) {
        let extensions = env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".EXE;.CMD;.BAT"));
        extensions
            .to_string_lossy()
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| directory.join(format!("{binary_name}{extension}")))
            .chain(std::iter::once(base))
            .collect()
    } else {
        vec![base]
    }
}

pub(super) fn first_non_empty_line(stdout: &[u8], stderr: &[u8]) -> String {
    [stdout, stderr]
        .into_iter()
        .flat_map(|stream| {
            String::from_utf8_lossy(stream)
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .map(|line| line.trim().to_owned())
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{find_executable_in_path, first_non_empty_line, version_arguments};
    use crate::discovery::{DiscoveryError, bundled_compatibility_manifest};

    #[test]
    fn version_output_prefers_stdout_and_falls_back_to_stderr() {
        assert_eq!(
            first_non_empty_line(b"\n harness 1.2.3 \n", b"harness 9.9.9\n"),
            "harness 1.2.3"
        );
        assert_eq!(
            first_non_empty_line(b"\n", b"\n harness 2.0.0 \n"),
            "harness 2.0.0"
        );
        assert_eq!(first_non_empty_line(b"", b""), "unknown");
    }

    #[test]
    fn version_command_requires_the_declared_executable_and_arguments() {
        let mut entry = bundled_compatibility_manifest()
            .expect("embedded manifest")
            .harnesses[0]
            .clone();
        entry.command = format!("{} --version --json", entry.id.binary_name());
        assert_eq!(
            version_arguments(&entry).expect("valid version command"),
            ["--version", "--json"]
        );

        entry.command = "other --version".to_owned();
        assert!(matches!(
            version_arguments(&entry),
            Err(DiscoveryError::InvalidVersionCommand { harness, command })
                if harness == entry.id && command == entry.command
        ));

        entry.command = entry.id.binary_name().to_owned();
        assert!(matches!(
            version_arguments(&entry),
            Err(DiscoveryError::InvalidVersionCommand { harness, command })
                if harness == entry.id && command == entry.command
        ));
    }

    #[cfg(unix)]
    #[test]
    fn executable_search_uses_path_order_and_requires_execute_permission() {
        use std::os::unix::fs::PermissionsExt;
        use std::{env, fs};

        let root = tempfile::tempdir().expect("temporary root");
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir_all(&first).expect("first directory");
        fs::create_dir_all(&second).expect("second directory");
        let rejected = first.join("harness");
        let selected = second.join("harness");
        fs::write(&rejected, "not executable").expect("rejected candidate");
        fs::write(&selected, "executable").expect("selected candidate");
        fs::set_permissions(&rejected, fs::Permissions::from_mode(0o600))
            .expect("rejected permissions");
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o700))
            .expect("selected permissions");
        let path = env::join_paths([first, second]).expect("test PATH");

        assert_eq!(
            find_executable_in_path("harness", Some(path.as_os_str())),
            Some(selected)
        );
    }
}
