#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn locate_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<PathBuf>, CliError> {
    locate_harness_for_platform(kind, arguments, cfg!(windows))
}

fn locate_harness_for_platform(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    windows: bool,
) -> Result<Option<PathBuf>, CliError> {
    // Upstream platform availability is independent of the installer catalog.
    // An explicit executable lets users try a future build without waiting for an update.
    if windows
        && matches!(kind, HarnessKind::PrimeAgent | HarnessKind::Fx)
        && arguments.executable.is_none()
    {
        return Err(CliError::HarnessWindowsUnavailable(kind));
    }
    match locate_harness_executable(kind, arguments.executable.as_deref()) {
        Ok(executable) => Ok(Some(executable)),
        Err(DiscoveryError::ExecutableNotFound(_))
            if install_spec(kind).is_some() && arguments.executable.is_none() =>
        {
            if let Some(executable) = executable_from_known_locations(kind) {
                return locate_harness_executable(kind, Some(&executable))
                    .map(Some)
                    .map_err(CliError::from);
            }
            if arguments.dry_run {
                eprintln!("{}", nan_harness_i18n::messages::discovery_was_not_found_on_path_dry_run_does_not_install_harnesses(nan_harness_i18n::locale(), &(kind)));
                eprintln!("{}", nan_harness_i18n::messages::discovery_run_nanh_doctor_after_installing_the_official_release(nan_harness_i18n::locale(), &(kind)));
                return Ok(None);
            }
            match offer_install(kind)? {
                InstallDecision::NotInteractive => {
                    report_install_skipped(kind, "installation requires an interactive terminal");
                    Err(DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned()).into())
                }
                InstallDecision::Declined => {
                    report_install_skipped(kind, "installation was declined");
                    Ok(None)
                }
                InstallDecision::Installed => {
                    let executable = executable_from_known_locations(kind);
                    match locate_harness_executable(kind, executable.as_deref()) {
                        Ok(executable) => Ok(Some(executable)),
                        Err(error @ DiscoveryError::ExecutableNotFound(_)) => {
                            eprintln!(
                                "{}", nan_harness_i18n::messages::discovery_was_installed_but_its_executable_is_not_visible_on_path(nan_harness_i18n::locale(), &(kind)));
                            Err(error.into())
                        }
                        Err(error) => Err(error.into()),
                    }
                }
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn discover_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<DiscoveryReport>, CliError> {
    let Some(executable) = locate_or_install_harness(kind, arguments)? else {
        return Ok(None);
    };
    inspect_harness(kind, &executable, discovery_options(arguments))
        .map(Some)
        .map_err(CliError::from)
}

pub(super) const fn discovery_options(arguments: &HarnessRunArgs) -> DiscoveryOptions {
    DiscoveryOptions {
        allow_unsupported: arguments.allow_unsupported,
        allow_untested: arguments.allow_untested,
    }
}

pub(super) fn report_install_skipped(kind: HarnessKind, reason: &str) {
    eprintln!(
        "{}",
        nan_harness_i18n::messages::discovery_was_not_found(
            nan_harness_i18n::locale(),
            &(kind),
            &(reason)
        )
    );
    eprintln!(
        "{}", nan_harness_i18n::messages::discovery_install_the_official_release_or_pass_executable_path_to(nan_harness_i18n::locale(), &(kind.binary_name())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn windows_upstream_availability_precedes_discovery_in_all_launch_modes() {
        for command in ["prime", "prime-agent", "fx"] {
            for flags in [
                vec![],
                vec!["--dry-run"],
                vec!["--allow-unsupported", "--allow-untested"],
            ] {
                let cli = Cli::try_parse_from(["nanh", command].into_iter().chain(flags))
                    .expect("valid harness command");
                let (kind, arguments) = harness_run_arguments(&cli).expect("harness arguments");
                assert!(matches!(
                    locate_harness_for_platform(kind, arguments, true),
                    Err(CliError::HarnessWindowsUnavailable(blocked)) if blocked == kind
                ));
            }
        }
    }

    #[test]
    fn explicit_future_builds_reach_executable_validation() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let missing = directory.path().join("future-build.exe");
        for command in ["prime", "fx"] {
            let cli = Cli::try_parse_from([
                "nanh",
                command,
                "--executable",
                missing.to_str().expect("UTF-8 path"),
            ])
            .expect("valid harness command");
            let (kind, arguments) = harness_run_arguments(&cli).expect("harness arguments");
            for windows in [false, true] {
                assert!(matches!(
                    locate_harness_for_platform(kind, arguments, windows),
                    Err(CliError::Discovery(_))
                ));
            }
        }
    }

    #[test]
    fn missing_installer_does_not_imply_upstream_windows_incompatibility() {
        for (command, windows) in [("goose", true), ("prime", false), ("fx", false)] {
            let cli =
                Cli::try_parse_from(["nanh", command, "--dry-run"]).expect("valid harness command");
            let (kind, arguments) = harness_run_arguments(&cli).expect("harness arguments");
            assert!(!matches!(
                locate_harness_for_platform(kind, arguments, windows),
                Err(CliError::HarnessWindowsUnavailable(_))
            ));
        }
    }
}
