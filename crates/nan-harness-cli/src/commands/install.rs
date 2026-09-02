mod catalog;
mod discovery;
mod error;
mod installer;
mod output;
mod post_install;
mod runtime;

pub(crate) use catalog::install_spec;
pub(crate) use discovery::executable_from_known_locations;
pub(crate) use error::InstallError;
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
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(InstallDecision::NotInteractive);
    }
    check_required_runtime(kind)?;

    let mut input = io::stdin().lock();
    let mut output = io::stderr().lock();
    writeln!(
        output,
        "{} was not found. Install the latest official release now?",
        spec.display_name()
    )
    .map_err(InstallError::Prompt)?;
    writeln!(
        output,
        "Official installer: {}",
        official_install_command(spec)?
    )
    .map_err(InstallError::Prompt)?;
    write!(output, "Install {} [y/N]: ", spec.display_name()).map_err(InstallError::Prompt)?;
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
    matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes")
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
