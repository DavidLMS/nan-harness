use crate::app::LocalDiagnosticsCommand;
use nan_harness_coordinator::{
    CoordinatorError, DiagnosticsStatus, disable_diagnostics, enable_diagnostics,
    purge_diagnostics, read_diagnostics_status,
};
use std::io::{IsTerminal as _, Write as _};

pub(crate) fn run(command: LocalDiagnosticsCommand) -> Result<(), CoordinatorError> {
    match command {
        LocalDiagnosticsCommand::On => print_enabled(&enable_diagnostics()?),
        LocalDiagnosticsCommand::Off => print_disabled(&disable_diagnostics()?),
        LocalDiagnosticsCommand::Status => print_status(&read_diagnostics_status()?),
        LocalDiagnosticsCommand::Purge { yes } => purge(yes)?,
    }
    Ok(())
}

fn print_enabled(status: &DiagnosticsStatus) {
    eprintln!(
        "{}", nan_harness_i18n::messages::local_diagnostics_warning_local_diagnostics_are_on_prompts_model_output_tool_data_and_embedde(nan_harness_i18n::locale()));
    eprintln!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_capture_directory(
            nan_harness_i18n::locale(),
            &(status.directory.display())
        )
    );
}

fn print_disabled(status: &DiagnosticsStatus) {
    print_recovery(status);
    eprintln!(
        "{}", nan_harness_i18n::messages::local_diagnostics_local_diagnostics_are_off_requests_already_being_captured_may_finish_writin(nan_harness_i18n::locale(), &(status.directory.display())));
}

fn print_recovery(status: &DiagnosticsStatus) {
    if status.recovered_settings {
        eprintln!(
            "{}", nan_harness_i18n::messages::local_diagnostics_invalid_diagnostic_settings_were_preserved_in_a_private_settings_backups_di(nan_harness_i18n::locale()));
    }
}

fn print_status(status: &DiagnosticsStatus) {
    println!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_local_diagnostics(
            nan_harness_i18n::locale(),
            &(if status.enabled { "on" } else { "off" })
        )
    );
    if let Some(capture_id) = &status.capture_id {
        println!(
            "{}",
            nan_harness_i18n::messages::local_diagnostics_capture(
                nan_harness_i18n::locale(),
                &(capture_id)
            )
        );
    }
    if let Some(enabled_at) = status.enabled_at_unix_seconds {
        println!(
            "{}",
            nan_harness_i18n::messages::local_diagnostics_enabled_at(
                nan_harness_i18n::locale(),
                &(format_timestamp(enabled_at))
            )
        );
    }
    println!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_directory(
            nan_harness_i18n::locale(),
            &(status.directory.display())
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_stored_bytes(
            nan_harness_i18n::locale(),
            &(status.bytes)
        )
    );
    println!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_incomplete_files(
            nan_harness_i18n::locale(),
            &(status.incomplete_files)
        )
    );
}

fn format_timestamp(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(|timestamp| time::OffsetDateTime::from_unix_timestamp(timestamp).ok())
        .and_then(|timestamp| {
            timestamp
                .format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| timestamp.to_string())
}

fn purge(yes: bool) -> Result<(), CoordinatorError> {
    if !yes && !confirm_purge()? {
        eprintln!(
            "{}",
            nan_harness_i18n::messages::local_diagnostics_diagnostic_logs_were_not_deleted(
                nan_harness_i18n::locale()
            )
        );
        return Ok(());
    }
    let status = purge_diagnostics()?;
    print_recovery(&status);
    eprintln!(
        "{}", nan_harness_i18n::messages::local_diagnostics_diagnostic_logs_were_deleted_local_diagnostics_are_off_coordinator_learning(nan_harness_i18n::locale(), &(status.directory.display())));
    Ok(())
}

fn confirm_purge() -> Result<bool, CoordinatorError> {
    if !std::io::stdin().is_terminal() {
        return Err(CoordinatorError::Protocol(
            "purge requires an interactive terminal or --yes",
        ));
    }
    eprint!(
        "{}",
        nan_harness_i18n::messages::local_diagnostics_delete_all_local_diagnostic_captures_y_n(
            nan_harness_i18n::locale()
        )
    );
    std::io::stderr()
        .flush()
        .map_err(|source| CoordinatorError::State {
            path: "stderr".into(),
            source,
        })?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|source| CoordinatorError::State {
            path: "stdin".into(),
            source,
        })?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
