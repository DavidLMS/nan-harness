use super::discovery::executable_from_known_locations;
use super::error::InstallError;
use super::output::{first_non_empty_output_line, summarize_output};
use nan_harness_core::HarnessKind;
use nan_harness_runtime::is_executable_file;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

const DSH_POST_INSTALL_CHECK: &[&str] = &["--profile", "web", "--help"];
const CLINE_POST_INSTALL_CHECK: &[&str] = &["--version"];
const OMP_POST_INSTALL_CHECK: &[&str] = &["--version"];

pub(super) fn post_install_check_arguments(kind: HarnessKind) -> Option<&'static [&'static str]> {
    match kind {
        HarnessKind::DeepSeekHarness => Some(DSH_POST_INSTALL_CHECK),
        HarnessKind::Cline => Some(CLINE_POST_INSTALL_CHECK),
        HarnessKind::Omp => Some(OMP_POST_INSTALL_CHECK),
        _ => None,
    }
}

pub(super) fn verify_post_install(kind: HarnessKind) -> Result<(), InstallError> {
    let Some(arguments) = post_install_check_arguments(kind) else {
        return Ok(());
    };
    let executable = executable_from_known_locations(kind).map_or_else(
        || kind.binary_name().to_owned(),
        |path| path.to_string_lossy().into_owned(),
    );
    verify_post_install_with_executable(kind, &executable, arguments)
}

fn verify_post_install_with_executable(
    kind: HarnessKind,
    executable: &str,
    arguments: &[&str],
) -> Result<(), InstallError> {
    let command = format!("{} {}", executable, arguments.join(" "));
    let isolated_home = TempDir::new().map_err(|source| InstallError::PostInstallCheckPrepare {
        harness: kind,
        source,
    })?;
    let mut check = Command::new(executable);
    check.args(arguments);
    if kind == HarnessKind::DeepSeekHarness {
        check
            .env("HOME", isolated_home.path())
            .env("USERPROFILE", isolated_home.path());
    }
    let output = check
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: kind,
            command: command.clone(),
            source,
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(InstallError::PostInstallCheckFailed {
        harness: kind,
        command,
        exit_code: output.status.code(),
        details: summarize_output(&output),
    })
}

pub(super) fn refresh_cline_binary_cache() -> Result<(), InstallError> {
    let root_command = "npm root --global";
    let root_output = Command::new("npm")
        .args(["root", "--global"])
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            source,
        })?;
    if !root_output.status.success() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            exit_code: root_output.status.code(),
            details: summarize_output(&root_output),
        });
    }

    let global_root = PathBuf::from(first_non_empty_output_line(&root_output));
    if !global_root.is_absolute() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            exit_code: None,
            details: "npm returned an invalid global package root".to_owned(),
        });
    }
    let package_root = global_root.join("cline");
    let postinstall = package_root.join("postinstall.mjs");
    let command = format!("node {}", postinstall.display());
    let output = Command::new("node")
        .arg(&postinstall)
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: HarnessKind::Cline,
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command,
            exit_code: output.status.code(),
            details: summarize_output(&output),
        });
    }

    if !cfg!(windows) {
        let cached_binary = package_root.join("bin/.cline");
        if !is_executable_file(&cached_binary) {
            return Err(InstallError::PostInstallCheckFailed {
                harness: HarnessKind::Cline,
                command,
                exit_code: None,
                details: format!(
                    "Cline postinstall did not create an executable cache at {}",
                    cached_binary.display()
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::post_install_check_arguments;
    use super::verify_post_install_with_executable;
    use nan_harness_core::HarnessKind;
    use std::fs;

    #[test]
    fn harnesses_with_fragile_installers_have_startup_checks() {
        assert_eq!(
            post_install_check_arguments(HarnessKind::DeepSeekHarness),
            Some(["--profile", "web", "--help"].as_slice())
        );
        assert_eq!(
            post_install_check_arguments(HarnessKind::Cline),
            Some(["--version"].as_slice())
        );
        assert_eq!(
            post_install_check_arguments(HarnessKind::Omp),
            Some(["--version"].as_slice())
        );
        assert_eq!(post_install_check_arguments(HarnessKind::ClaudeCode), None);
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_post_install_check_uses_an_isolated_home() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary root should exist");
        let executable = root.path().join("dsh");
        let real_home = std::env::var("HOME").expect("test HOME should exist");
        assert!(!real_home.contains(['\"', '\n', '\r']));
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$HOME\" != \"{real_home}\" ] || exit 29\nmkdir -p \"$HOME/.dsh\"\ntouch \"$HOME/.dsh/post-install-check\"\n"
            ),
        )
        .expect("fake DSH should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fake DSH should be executable");

        verify_post_install_with_executable(
            HarnessKind::DeepSeekHarness,
            executable.to_string_lossy().as_ref(),
            &["--profile", "web", "--help"],
        )
        .expect("post-install check should use an isolated home");
    }
}
