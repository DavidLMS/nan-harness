#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn locate_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<PathBuf>, CliError> {
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
