use super::*;

pub(crate) fn discover_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<DiscoveryReport>, CliError> {
    let options = DiscoveryOptions {
        allow_unsupported: arguments.allow_unsupported,
        allow_untested: arguments.allow_untested,
    };
    match discover_harness(kind, arguments.executable.as_deref(), options) {
        Ok(report) => Ok(Some(report)),
        Err(DiscoveryError::ExecutableNotFound(_))
            if install_spec(kind).is_some() && arguments.executable.is_none() =>
        {
            if let Some(executable) = executable_from_known_locations(kind) {
                return discover_harness(kind, Some(&executable), options)
                    .map(Some)
                    .map_err(CliError::from);
            }
            if arguments.dry_run {
                eprintln!("{kind} was not found on PATH; dry-run does not install harnesses.");
                eprintln!("Run `nan doctor {kind}` after installing the official release.");
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
                    match discover_harness(kind, executable.as_deref(), options) {
                        Ok(report) => Ok(Some(report)),
                        Err(error @ DiscoveryError::ExecutableNotFound(_)) => {
                            eprintln!(
                                "{kind} was installed, but its executable is not visible on PATH."
                            );
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

pub(super) fn report_install_skipped(kind: HarnessKind, reason: &str) {
    eprintln!("{kind} was not found; {reason}.");
    eprintln!(
        "Install the official release, or pass --executable /path/to/{}.",
        kind.binary_name()
    );
}
