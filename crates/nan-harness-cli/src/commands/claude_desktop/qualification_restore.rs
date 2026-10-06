//! Hosted-only restoration facts never serialize error messages or state paths.
use super::{ClaudeDesktopError, DesktopPaths, qualification_config};
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Stage {
    SessionLock,
    ProcessCheck,
    Receipt,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    stage: Stage,
    outcome: &'static str,
    error_category: Option<&'static str>,
}

fn observation(stage: Stage, error: Option<&ClaudeDesktopError>) -> Observation {
    let category = error.map(|error| match error {
        ClaudeDesktopError::ConcurrentSession => "session-busy",
        ClaudeDesktopError::AlreadyRunning => "app-running",
        ClaudeDesktopError::ProcessCheck(_) | ClaudeDesktopError::ProcessCheckFailed(_) => {
            "process-query"
        }
        ClaudeDesktopError::UnsafeSymlink => "unsafe-state",
        ClaudeDesktopError::BackupHashMismatch => "backup-mismatch",
        ClaudeDesktopError::ParseReceipt(_) | ClaudeDesktopError::UnsupportedReceipt => {
            "receipt-schema"
        }
        ClaudeDesktopError::NoReceipt => "no-receipt",
        ClaudeDesktopError::Permissions(_) => "permissions",
        ClaudeDesktopError::Lock(_) => "lock-io",
        ClaudeDesktopError::ReadReceipt(_) => "receipt-read",
        ClaudeDesktopError::ReadBackup(_) => "backup-read",
        ClaudeDesktopError::Restore(_) => "document-restore",
        ClaudeDesktopError::RemoveBackup(_) => "backup-remove",
        ClaudeDesktopError::RemoveReceipt(_) => "receipt-remove",
        ClaudeDesktopError::ReadConfig(_) => "state-read",
        ClaudeDesktopError::Write(_) => "state-write",
        ClaudeDesktopError::CreateDirectory(_) => "directory-create",
        ClaudeDesktopError::OrphanBackup => "orphan-backup",
        _ => "other",
    });
    Observation {
        schema_version: 1,
        mechanism: "claude-restore",
        diagnostics_only: true,
        stage,
        outcome: match category {
            None => "restored",
            Some("no-receipt") => "nothing-to-restore",
            _ => "rejected",
        },
        error_category: category,
    }
}

pub(super) fn record(paths: &DesktopPaths, stage: Stage, error: Option<&ClaudeDesktopError>) {
    let Some(directory) = qualification_config::observation_directory(paths) else {
        return;
    };
    let path = directory.join(format!("claude-restore-{}.json", std::process::id()));
    if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
        let _ = serde_json::to_writer(file, &observation(stage, error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restoration_distinguishes_busy_process_and_io_without_serializing_details() {
        for (stage, error, expected) in [
            (
                Stage::SessionLock,
                ClaudeDesktopError::ConcurrentSession,
                "session-busy",
            ),
            (
                Stage::ProcessCheck,
                ClaudeDesktopError::AlreadyRunning,
                "app-running",
            ),
            (
                Stage::Receipt,
                ClaudeDesktopError::Restore(std::io::Error::other("PRIVATE path")),
                "document-restore",
            ),
            (
                Stage::Receipt,
                ClaudeDesktopError::BackupHashMismatch,
                "backup-mismatch",
            ),
        ] {
            let value = serde_json::to_value(observation(stage, Some(&error))).unwrap();
            assert_eq!(value["errorCategory"], expected);
            assert_eq!(value["outcome"], "rejected");
            assert!(!value.to_string().contains("PRIVATE"));
        }
        let absent = observation(Stage::Receipt, Some(&ClaudeDesktopError::NoReceipt));
        assert_eq!(absent.outcome, "nothing-to-restore");
        assert_eq!(observation(Stage::Receipt, None).outcome, "restored");
    }
}
