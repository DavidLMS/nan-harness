mod catalog;
mod discovery;
mod error;
mod gentle_shell;
mod installer;
mod output;
mod post_install;
mod runtime;
mod source;

pub(crate) use catalog::install_spec;
pub(crate) use discovery::executable_from_known_locations;
pub(crate) use error::InstallError;
pub(crate) use gentle_shell::{locate_gentle_shell, locate_or_install_gentle_shell};
pub(crate) use runtime::check_required_runtime;

use crate::commands::install::catalog::official_install_command;
use nan_harness_core::HarnessKind;
use std::io::{self, BufRead, IsTerminal, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallDecision {
    NotInteractive,
    Declined,
    Installed,
}

pub(crate) fn offer_install(kind: HarnessKind) -> Result<InstallDecision, InstallError> {
    let spec = install_spec(kind).ok_or(InstallError::UnsupportedHarness(kind))?;
    offer_install_spec(spec, || Ok(()))
}

fn offer_install_spec(
    spec: &catalog::InstallSpec,
    extra_prerequisites: impl FnOnce() -> Result<(), InstallError>,
) -> Result<InstallDecision, InstallError> {
    let kind = spec.kind();
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(InstallDecision::NotInteractive);
    }
    check_required_runtime(kind)?;
    installer::check_install_prerequisites(spec)?;
    extra_prerequisites()?;

    let mut input = io::stdin().lock();
    let mut output = io::stderr().lock();
    writeln!(
        output,
        "{}",
        if kind == HarnessKind::ZCode {
            nan_harness_i18n::messages::install_source_offer(nan_harness_i18n::locale())
        } else { nan_harness_i18n::messages::install_was_not_found_install_the_latest_official_release_now(
            nan_harness_i18n::locale(),
            &(spec.display_name())
        ) }
    )
    .map_err(InstallError::Prompt)?;
    writeln!(
        output,
        "{}",
        if kind == HarnessKind::ZCode {
            nan_harness_i18n::messages::install_source_commands(
                nan_harness_i18n::locale(),
                &official_install_command(spec)?,
            )
        } else {
            nan_harness_i18n::messages::install_official_installer(
                nan_harness_i18n::locale(),
                &(official_install_command(spec)?),
            )
        }
    )
    .map_err(InstallError::Prompt)?;
    write!(
        output,
        "{}",
        nan_harness_i18n::messages::install_install_y_n(
            nan_harness_i18n::locale(),
            &(spec.display_name())
        )
    )
    .map_err(InstallError::Prompt)?;
    output.flush().map_err(InstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(InstallError::Prompt)?;
    if !is_affirmative(&response) {
        return Ok(InstallDecision::Declined);
    }

    installer::install(spec)?;
    post_install::verify_post_install(kind)?;
    Ok(InstallDecision::Installed)
}

fn is_affirmative(response: &str) -> bool {
    nan_harness_i18n::yes_no(nan_harness_i18n::locale(), response) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::is_affirmative;

    #[test]
    fn missing_and_declined_install_responses_are_nonfatal() {
        assert!(!is_affirmative(""));
        assert!(!is_affirmative("no"));
        assert!(!is_affirmative("N"));
        assert!(is_affirmative("y"));
        assert!(is_affirmative("YES\n"));
    }
}
