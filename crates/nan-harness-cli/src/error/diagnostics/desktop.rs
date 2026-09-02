use super::details;
use crate::commands::chatgpt_desktop::ChatGptDesktopError;
use crate::commands::claude_desktop::ClaudeDesktopError;
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticOperation, DiagnosticReason};

pub(super) fn chatgpt(error: &ChatGptDesktopError) -> Diagnostic {
    match error {
        ChatGptDesktopError::UnsupportedPlatform
        | ChatGptDesktopError::OlderUnsupported { .. }
        | ChatGptDesktopError::NewerUntested { .. }
        | ChatGptDesktopError::Compatibility(_) => {
            Diagnostic::general(DiagnosticReason::UnsupportedVersion)
        }
        ChatGptDesktopError::AppNotFound => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        ChatGptDesktopError::InvalidInstallation => {
            Diagnostic::general(DiagnosticReason::InvalidExecutable)
        }
        ChatGptDesktopError::VersionCommand(source) => {
            details::io(DiagnosticOperation::RunVersionCommand, source)
        }
        ChatGptDesktopError::VersionCommandFailed => details::process(
            DiagnosticReason::ProcessExited,
            DiagnosticOperation::RunVersionCommand,
            None,
        ),
        ChatGptDesktopError::UnparseableVersion => {
            Diagnostic::general(DiagnosticReason::UnparseableVersion)
        }
        ChatGptDesktopError::AppAlreadyRunning
        | ChatGptDesktopError::SingletonRace
        | ChatGptDesktopError::UnmanagedProfile
        | ChatGptDesktopError::InvalidMarker
        | ChatGptDesktopError::InvalidReceipt
        | ChatGptDesktopError::OrphanedSessionFiles => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        ChatGptDesktopError::AppDidNotTerminate | ChatGptDesktopError::StopApp(_) => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
        ChatGptDesktopError::AppExitedDuringStartup => {
            Diagnostic::general(DiagnosticReason::ProcessExited)
        }
        ChatGptDesktopError::InspectProcess(source) => {
            details::io(DiagnosticOperation::WaitForHarness, source)
        }
        ChatGptDesktopError::ProcessInspectionFailed | ChatGptDesktopError::WaitForApp(_) => {
            Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
        }
        ChatGptDesktopError::State(_) | ChatGptDesktopError::Persistence(_) => {
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
        }
        ChatGptDesktopError::InspectProfile(source) | ChatGptDesktopError::ReadState(source) => {
            details::io(DiagnosticOperation::ReadConfiguration, source)
        }
        ChatGptDesktopError::WriteState(source) => {
            details::io(DiagnosticOperation::WriteConfiguration, source)
        }
        ChatGptDesktopError::ParseMarker(_) | ChatGptDesktopError::ParseReceipt(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        ChatGptDesktopError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        ChatGptDesktopError::Bridge(_) | ChatGptDesktopError::BridgeExited => {
            Diagnostic::general(DiagnosticReason::BridgeExited)
        }
        ChatGptDesktopError::BridgeHandshakeTimeout => {
            Diagnostic::general(DiagnosticReason::AuthenticationRejected)
        }
        ChatGptDesktopError::StartApp(source) => {
            details::io(DiagnosticOperation::StartHarness, source)
        }
    }
}

pub(super) fn claude(error: &ClaudeDesktopError) -> Diagnostic {
    match error {
        ClaudeDesktopError::UnsupportedPlatform | ClaudeDesktopError::Compatibility(_) => {
            Diagnostic::general(DiagnosticReason::UnsupportedVersion)
        }
        ClaudeDesktopError::AppNotFound { .. } => {
            Diagnostic::general(DiagnosticReason::MissingExecutable)
        }
        ClaudeDesktopError::AlreadyRunning
        | ClaudeDesktopError::ConcurrentSession
        | ClaudeDesktopError::OrphanReceipt
        | ClaudeDesktopError::NoReceipt
        | ClaudeDesktopError::UnsafeSymlink
        | ClaudeDesktopError::OrphanBackup
        | ClaudeDesktopError::BackupHashMismatch
        | ClaudeDesktopError::UnsupportedReceipt => {
            Diagnostic::general(DiagnosticReason::ConfigurationConflict)
        }
        ClaudeDesktopError::DidNotStart => {
            Diagnostic::general(DiagnosticReason::ProcessStartFailed)
        }
        ClaudeDesktopError::DidNotTerminate => {
            Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
        }
        ClaudeDesktopError::Bridge(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
        ClaudeDesktopError::MissingHome
        | ClaudeDesktopError::MissingPlatformDirectory(_)
        | ClaudeDesktopError::InvalidStatePath => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        ClaudeDesktopError::CreateDirectory(source)
        | ClaudeDesktopError::Permissions(source)
        | ClaudeDesktopError::CreateBackupDirectory(source)
        | ClaudeDesktopError::WriteBackup(source)
        | ClaudeDesktopError::Write(source) => {
            details::io(DiagnosticOperation::WriteConfiguration, source)
        }
        ClaudeDesktopError::Lock(source)
        | ClaudeDesktopError::ReadConfig(source)
        | ClaudeDesktopError::ReadBackup(source)
        | ClaudeDesktopError::ReadReceipt(source) => {
            details::io(DiagnosticOperation::ReadConfiguration, source)
        }
        ClaudeDesktopError::ProcessCheck(source) => {
            details::io(DiagnosticOperation::WaitForHarness, source)
        }
        ClaudeDesktopError::ProcessCheckFailed(exit_code) => details::process(
            DiagnosticReason::ProcessWaitFailed,
            DiagnosticOperation::WaitForHarness,
            *exit_code,
        ),
        ClaudeDesktopError::Launch(source) => {
            details::io(DiagnosticOperation::StartHarness, source)
        }
        ClaudeDesktopError::LaunchFailed(exit_code) => details::process(
            DiagnosticReason::ProcessStartFailed,
            DiagnosticOperation::StartHarness,
            *exit_code,
        ),
        ClaudeDesktopError::Terminate(source) => {
            details::io(DiagnosticOperation::StopHarness, source)
        }
        ClaudeDesktopError::TerminateFailed(exit_code) => details::process(
            DiagnosticReason::ProcessTerminationFailed,
            DiagnosticOperation::StopHarness,
            *exit_code,
        ),
        ClaudeDesktopError::ParseConfig(_)
        | ClaudeDesktopError::ConfigRoot
        | ClaudeDesktopError::ParseReceipt(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        ClaudeDesktopError::SerializeConfig(_) | ClaudeDesktopError::SerializeReceipt(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        ClaudeDesktopError::Restore(source)
        | ClaudeDesktopError::RemoveBackup(source)
        | ClaudeDesktopError::RemoveReceipt(source) => {
            details::io(DiagnosticOperation::RemoveConfiguration, source)
        }
    }
}
