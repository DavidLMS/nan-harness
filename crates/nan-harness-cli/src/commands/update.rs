use nan_harness_runtime::update::{ReleaseManifest, UpdateError, UpdateManager};
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateChoice {
    Install,
    Defer,
    Skip,
}

pub(crate) async fn check_on_start(interactive: bool) -> Result<Option<i32>, UpdateError> {
    if !interactive || !UpdateManager::automatic_checks_enabled() {
        return Ok(None);
    }
    let manager = UpdateManager::from_environment()?;
    if !manager.channel_available() {
        return Ok(None);
    }
    let Some(release) = manager.recommended_release(false, true).await? else {
        return Ok(None);
    };

    let choice = {
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stderr().lock();
        prompt(&release, manager.current_version(), &mut input, &mut output)?
    };
    match choice {
        UpdateChoice::Install => {
            eprintln!(
                "{}",
                nan_harness_i18n::messages::update_updating_nan_harness(
                    nan_harness_i18n::locale(),
                    &(manager.current_version()),
                    &(release.version)
                )
            );
            manager.install(&release).await?;
            eprintln!(
                "{}",
                nan_harness_i18n::messages::update_nan_harness_installed_restarting_your_command(
                    nan_harness_i18n::locale(),
                    &(release.version)
                )
            );
            restart_current_command().map(Some)
        }
        UpdateChoice::Defer => Ok(None),
        UpdateChoice::Skip => {
            manager.skip(release.version)?;
            Ok(None)
        }
    }
}

pub(crate) async fn run_manual() -> Result<(), UpdateError> {
    let manager = UpdateManager::from_environment()?;
    let Some(release) = manager.available_release().await? else {
        println!(
            "{}",
            nan_harness_i18n::messages::update_nan_harness_is_up_to_date_keep_building(
                nan_harness_i18n::locale(),
                &(manager.current_version())
            )
        );
        return Ok(());
    };
    println!(
        "{}",
        nan_harness_i18n::messages::update_updating_nan_harness(
            nan_harness_i18n::locale(),
            &(manager.current_version()),
            &(release.version)
        )
    );
    manager.install(&release).await?;
    println!(
        "{}",
        nan_harness_i18n::messages::update_nan_harness_installed_ready_to_build(
            nan_harness_i18n::locale(),
            &(release.version)
        )
    );
    Ok(())
}

fn prompt(
    release: &ReleaseManifest,
    current_version: &semver::Version,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<UpdateChoice, UpdateError> {
    writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::update_new_nan_harness_release(
            nan_harness_i18n::locale(),
            &(current_version),
            &(release.version)
        )
    )
    .map_err(UpdateError::Prompt)?;
    writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::update_release_notes(
            nan_harness_i18n::locale(),
            &(release.notes_url)
        )
    )
    .map_err(UpdateError::Prompt)?;
    writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::update_1_update_now(nan_harness_i18n::locale())
    )
    .map_err(UpdateError::Prompt)?;
    writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::update_2_not_now(nan_harness_i18n::locale())
    )
    .map_err(UpdateError::Prompt)?;
    writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::update_3_skip_version(
            nan_harness_i18n::locale(),
            &(release.version)
        )
    )
    .map_err(UpdateError::Prompt)?;
    write!(
        output,
        "{}",
        nan_harness_i18n::messages::update_select_an_option_1_3_default_not_now_press_enter(
            nan_harness_i18n::locale()
        )
    )
    .map_err(UpdateError::Prompt)?;
    output.flush().map_err(UpdateError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(UpdateError::Prompt)?;
    Ok(parse_choice(&response))
}

fn parse_choice(value: &str) -> UpdateChoice {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "u" | "update" => UpdateChoice::Install,
        "3" | "s" | "skip" => UpdateChoice::Skip,
        _ => UpdateChoice::Defer,
    }
}

fn restart_current_command() -> Result<i32, UpdateError> {
    let executable = std::env::current_exe().map_err(UpdateError::Restart)?;
    let arguments = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    let status = Command::new(executable)
        .args(arguments)
        .status()
        .map_err(UpdateError::Restart)?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::{UpdateChoice, parse_choice, prompt};
    use nan_harness_runtime::update::{ReleaseArtifact, ReleaseManifest};
    use semver::Version;
    use std::io::Cursor;

    #[test]
    fn prompt_offers_install_defer_and_exact_version_skip() {
        let release = release();
        let mut input = Cursor::new(b"3\n");
        let mut output = Vec::new();

        let choice = prompt(&release, &Version::new(0, 1, 0), &mut input, &mut output)
            .expect("prompt should complete");
        let output = String::from_utf8(output).expect("prompt should be UTF-8");

        assert_eq!(choice, UpdateChoice::Skip);
        assert!(output.contains("🚀 New nan-harness release: 0.1.0 -> 0.2.0"));
        assert!(output.contains("1. Update now"));
        assert!(output.contains("2. Not now"));
        assert!(output.contains("3. Skip version 0.2.0"));
        assert!(output.contains("default: Not now; press Enter"));
    }

    #[test]
    fn empty_and_unknown_choices_defer_safely() {
        for value in ["", "\n", "2", "not now", "invalid"] {
            assert_eq!(parse_choice(value), UpdateChoice::Defer);
        }
        assert_eq!(parse_choice("1"), UpdateChoice::Install);
        assert_eq!(parse_choice("skip"), UpdateChoice::Skip);
    }

    fn release() -> ReleaseManifest {
        ReleaseManifest {
            schema_version: 1,
            version: Version::new(0, 2, 0),
            notes_url: "https://example.com/releases/0.2.0".to_owned(),
            artifacts: vec![ReleaseArtifact {
                target: "aarch64-apple-darwin".to_owned(),
                url: "https://example.com/nan".to_owned(),
                sha256: "0".repeat(64),
            }],
        }
    }
}
