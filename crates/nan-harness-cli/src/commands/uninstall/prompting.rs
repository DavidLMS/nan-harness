use super::UninstallError;
use super::installation::InstallationPaths;
use crate::commands::persistence::PersistentIntegration;
use nan_harness_core::HarnessKind;
use std::io::{BufRead, Write};
use std::path::Path;

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub(super) fn prompt(
    installation: &InstallationPaths,
    data_directory: &Path,
    integrations: &[PersistentIntegration],
    native_configurations: &[HarnessKind],
    has_saved_credential: bool,
    has_chatgpt_profile: bool,
    has_hermes_profile: bool,
    has_pen_configuration: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<bool, UninstallError> {
    writeln!(output, "\nnan-harness will remove:").map_err(UninstallError::Prompt)?;
    if integrations.is_empty() && native_configurations.is_empty() {
        writeln!(output, "  - Managed harness configurations: none")
            .map_err(UninstallError::Prompt)?;
    } else {
        let names = native_configurations
            .iter()
            .map(ToString::to_string)
            .chain(integrations.iter().map(ToString::to_string))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(output, "  - Managed harness configurations: {names}")
            .map_err(UninstallError::Prompt)?;
    }
    let credential = if has_saved_credential { "yes" } else { "none" };
    writeln!(output, "  - Saved NaN API key: {credential}").map_err(UninstallError::Prompt)?;
    if has_chatgpt_profile {
        writeln!(
            output,
            "  - ChatGPT Desktop profile: authentication, history, and cache"
        )
        .map_err(UninstallError::Prompt)?;
    }
    if has_hermes_profile {
        writeln!(
            output,
            "  - Hermes CLI/Desktop shared profile: conversations and local state"
        )
        .map_err(UninstallError::Prompt)?;
    }
    if has_pen_configuration {
        writeln!(output, "  - Pen Desktop native NaN provider and copied key")
            .map_err(UninstallError::Prompt)?;
    }
    writeln!(
        output,
        "  - Application data: '{}'",
        data_directory.display()
    )
    .map_err(UninstallError::Prompt)?;
    writeln!(
        output,
        "  - Executable: '{}'",
        installation.executable_path.display()
    )
    .map_err(UninstallError::Prompt)?;
    if installation.remove_alias {
        writeln!(output, "  - Alias: '{}'", installation.alias_path.display())
            .map_err(UninstallError::Prompt)?;
    }
    write!(output, "\nContinue? [y/N]: ").map_err(UninstallError::Prompt)?;
    output.flush().map_err(UninstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(UninstallError::Prompt)?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::{InstallationPaths, prompt};
    use crate::commands::persistence::PersistentIntegration;
    use std::io::Cursor;
    use std::path::PathBuf;

    #[test]
    fn prompt_defaults_to_preserving_the_installation() {
        for response in ["", "\n", "n\n", "anything\n"] {
            let mut input = Cursor::new(response.as_bytes());
            let mut output = Vec::new();
            assert!(
                !prompt(
                    &installation(),
                    std::path::Path::new("/tmp/state"),
                    &[PersistentIntegration::Pi, PersistentIntegration::Aider],
                    &[],
                    true,
                    false,
                    false,
                    false,
                    &mut input,
                    &mut output,
                )
                .expect("prompt should complete")
            );
        }
    }

    #[test]
    fn prompt_accepts_only_explicit_confirmation() {
        for response in ["y\n", "Y\n", "yes\n", "YES\n"] {
            let mut input = Cursor::new(response.as_bytes());
            let mut output = Vec::new();
            assert!(
                prompt(
                    &installation(),
                    std::path::Path::new("/tmp/state"),
                    &[PersistentIntegration::Pi],
                    &[],
                    false,
                    false,
                    false,
                    false,
                    &mut input,
                    &mut output,
                )
                .expect("prompt should complete")
            );
        }
    }

    fn installation() -> InstallationPaths {
        InstallationPaths {
            executable_path: PathBuf::from("/tmp/bin/nan-harness"),
            alias_path: PathBuf::from("/tmp/bin/nanh"),
            remove_alias: true,
            #[cfg(windows)]
            user_path_entry_added: false,
        }
    }
}
