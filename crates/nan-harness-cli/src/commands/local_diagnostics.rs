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
        "Warning: local diagnostics are ON. Prompts, model output, tool data, and embedded attachments will be stored as unencrypted private files. Logs are never uploaded and are not deleted automatically."
    );
    eprintln!("Capture directory: {}", status.directory.display());
}

fn print_disabled(status: &DiagnosticsStatus) {
    eprintln!(
        "Local diagnostics are OFF. Requests already being captured may finish writing. Existing logs remain at {}.",
        status.directory.display()
    );
}

fn print_status(status: &DiagnosticsStatus) {
    println!(
        "Local diagnostics: {}",
        if status.enabled { "on" } else { "off" }
    );
    if let Some(capture_id) = &status.capture_id {
        println!("Capture: {capture_id}");
    }
    if let Some(enabled_at) = status.enabled_at_unix_seconds {
        println!("Enabled at: {}", format_timestamp(enabled_at));
    }
    println!("Directory: {}", status.directory.display());
    println!("Stored bytes: {}", status.bytes);
    println!("Incomplete files: {}", status.incomplete_files);
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
        eprintln!("Diagnostic logs were not deleted.");
        return Ok(());
    }
    let status = purge_diagnostics()?;
    eprintln!(
        "Diagnostic logs were deleted. Local diagnostics are off; coordinator learning was preserved. Diagnostic state remains at {}.",
        status.directory.display(),
    );
    Ok(())
}

fn confirm_purge() -> Result<bool, CoordinatorError> {
    if !std::io::stdin().is_terminal() {
        return Err(CoordinatorError::Protocol(
            "purge requires an interactive terminal or --yes",
        ));
    }
    eprint!("Delete all local diagnostic captures? [y/N] ");
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
