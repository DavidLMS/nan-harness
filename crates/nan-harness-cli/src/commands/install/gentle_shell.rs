use super::catalog::{InstallSpec, command};
use super::{InstallDecision, discovery, offer_install_spec};
use crate::app::HarnessRunArgs;
use crate::error::CliError;
use nan_harness_core::HarnessKind;
use nan_harness_runtime::{DiscoveryError, locate_harness_executable, locate_launcher};
use std::path::PathBuf;

// Gentle Shell is a Pi launcher, not a separate provider adapter.
const SPEC: InstallSpec = InstallSpec {
    kind: HarnessKind::Pi,
    display_name: "Gentle Shell",
    official_url: "https://github.com/Gentleman-Programming/gentle-shell",
    unix: command("npm", &["install", "--global", "gentle-pi@latest"]),
    windows: Some(command("npm", &["install", "--global", "gentle-pi@latest"])),
};

pub(crate) fn locate_gentle_shell() -> Result<PathBuf, DiscoveryError> {
    match locate_launcher("gentle-shell") {
        Err(DiscoveryError::ExecutableNotFound(_)) => {
            discovery::npm_launcher_from_known_locations("gentle-shell")
                .ok_or_else(|| DiscoveryError::ExecutableNotFound("gentle-shell".to_owned()))
        }
        result => result,
    }
}

pub(crate) fn locate_or_install_gentle_shell(
    arguments: &HarnessRunArgs,
) -> Result<Option<PathBuf>, CliError> {
    if let Some(executable) = &arguments.executable {
        return locate_harness_executable(HarnessKind::Pi, Some(executable))
            .map(Some)
            .map_err(CliError::from);
    }
    locate_or_install(arguments.dry_run, locate_gentle_shell, || {
        offer_install_spec(&SPEC)
    })
}

fn locate_or_install(
    dry_run: bool,
    mut locate: impl FnMut() -> Result<PathBuf, DiscoveryError>,
    offer: impl FnOnce() -> Result<InstallDecision, super::InstallError>,
) -> Result<Option<PathBuf>, CliError> {
    match locate() {
        Ok(path) => return Ok(Some(path)),
        Err(DiscoveryError::ExecutableNotFound(_)) => (),
        Err(error) => return Err(error.into()),
    }
    eprintln!(
        "{}",
        nan_harness_i18n::messages::install_gentle_shell_missing(nan_harness_i18n::locale())
    );
    if dry_run {
        return Ok(None);
    }
    match offer()? {
        InstallDecision::Declined => Ok(None),
        InstallDecision::NotInteractive => {
            Err(DiscoveryError::ExecutableNotFound("gentle-shell".to_owned()).into())
        }
        // Launch discovery validates both the launcher and embedded Pi versions.
        InstallDecision::Installed => locate().map(Some).map_err(|error| {
            super::InstallError::PostInstallCheckStart {
                harness: HarnessKind::Pi,
                command: "gentle-shell --version".to_owned(),
                source: std::io::Error::other(error),
            }
            .into()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn missing() -> Result<PathBuf, DiscoveryError> {
        Err(DiscoveryError::ExecutableNotFound(
            "gentle-shell".to_owned(),
        ))
    }

    #[test]
    fn installer_uses_the_launcher_package_on_both_platforms() {
        assert_eq!(SPEC.windows, Some(SPEC.unix));
        assert_eq!(
            super::super::catalog::official_install_command(&SPEC).unwrap(),
            "npm install --global gentle-pi@latest"
        );
    }

    #[test]
    fn dry_run_and_existing_launcher_never_offer_installation() {
        assert!(
            locate_or_install(true, missing, || panic!("must not install"))
                .unwrap()
                .is_none()
        );
        let path = PathBuf::from("installed-gentle-shell");
        assert_eq!(
            locate_or_install(false, || Ok(path.clone()), || panic!("must not install")).unwrap(),
            Some(path)
        );
    }

    #[test]
    fn refusal_is_nonfatal_and_noninteractive_missing_is_actionable() {
        assert!(
            locate_or_install(false, missing, || Ok(InstallDecision::Declined))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            locate_or_install(false, missing, || Ok(InstallDecision::NotInteractive)),
            Err(CliError::Discovery(DiscoveryError::ExecutableNotFound(_)))
        ));
    }

    #[test]
    fn successful_installation_rediscovers_and_failure_propagates() {
        let calls = Cell::new(0);
        let path = PathBuf::from("new-gentle-shell");
        let result = locate_or_install(
            false,
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    missing()
                } else {
                    Ok(path.clone())
                }
            },
            || Ok(InstallDecision::Installed),
        )
        .unwrap();
        assert_eq!(result, Some(path));
        assert_eq!(calls.get(), 2);
        assert!(matches!(
            locate_or_install(false, missing, || Err(
                super::super::InstallError::CommandFailed {
                    harness: HarnessKind::Pi,
                    program: "npm",
                    exit_code: Some(1),
                }
            )),
            Err(CliError::Install(_))
        ));
        assert!(matches!(
            locate_or_install(false, missing, || Ok(InstallDecision::Installed)),
            Err(CliError::Install(
                super::super::InstallError::PostInstallCheckStart { .. }
            ))
        ));
    }
}
