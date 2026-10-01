use super::UninstallError;
use super::installation::InstallationPaths;
use crate::commands::persistence::PersistentIntegration;
use nan_harness_core::HarnessKind;
use nan_harness_i18n::{locale, messages};
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
    writeln!(
        output,
        "{}",
        messages::prompting_nan_harness_will_remove(locale())
    )
    .map_err(UninstallError::Prompt)?;
    if integrations.is_empty() && native_configurations.is_empty() {
        writeln!(
            output,
            "{}",
            messages::prompting_managed_harness_configurations_none(locale())
        )
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
        writeln!(
            output,
            "{}",
            messages::prompting_managed_harness_configurations(locale(), &(names))
        )
        .map_err(UninstallError::Prompt)?;
    }
    let credential = if has_saved_credential { "yes" } else { "none" };
    writeln!(
        output,
        "{}",
        messages::prompting_saved_nan_api_key(locale(), &(credential))
    )
    .map_err(UninstallError::Prompt)?;
    if has_chatgpt_profile {
        writeln!(
            output,
            "{}",
            messages::prompting_chatgpt_desktop_profile_authentication_history_and_cache(locale())
        )
        .map_err(UninstallError::Prompt)?;
    }
    if has_hermes_profile {
        writeln!(
            output,
            "{}",
            messages::prompting_hermes_cli_desktop_shared_profile_conversations_and_local_state(
                locale()
            )
        )
        .map_err(UninstallError::Prompt)?;
    }
    if has_pen_configuration {
        writeln!(
            output,
            "{}",
            messages::prompting_pen_desktop_native_nan_provider_and_copied_key(locale())
        )
        .map_err(UninstallError::Prompt)?;
    }
    writeln!(
        output,
        "{}",
        messages::prompting_application_data(locale(), &(data_directory.display()))
    )
    .map_err(UninstallError::Prompt)?;
    writeln!(
        output,
        "{}",
        messages::prompting_executable(locale(), &(installation.executable_path.display()))
    )
    .map_err(UninstallError::Prompt)?;
    if installation.remove_alias {
        writeln!(
            output,
            "{}",
            messages::prompting_alias(locale(), &(installation.alias_path.display()))
        )
        .map_err(UninstallError::Prompt)?;
    }
    write!(output, "{}", messages::prompting_continue_y_n(locale()))
        .map_err(UninstallError::Prompt)?;
    output.flush().map_err(UninstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(UninstallError::Prompt)?;
    Ok(nan_harness_i18n::yes_no(locale(), &response) == Some(true))
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

#[cfg(test)]
mod disclosure_tests;
