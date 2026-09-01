use super::*;

pub(crate) fn filesystem_diagnostic(error: &HermesDesktopError) -> Diagnostic {
    match error {
        HermesDesktopError::Serialize(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        HermesDesktopError::MissingStateDirectory | HermesDesktopError::MissingHomeDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        HermesDesktopError::CreateStateDirectory(source)
        | HermesDesktopError::ProtectStateDirectory(source)
        | HermesDesktopError::OpenLock(source)
        | HermesDesktopError::ProtectLock(source)
        | HermesDesktopError::Lock(source)
        | HermesDesktopError::CreateProfile(source)
        | HermesDesktopError::ProtectProfile(source)
        | HermesDesktopError::CreateParkingDirectory(source)
        | HermesDesktopError::ProtectParkingDirectory(source)
        | HermesDesktopError::ActivateProfile(source)
        | HermesDesktopError::ParkProfile(source)
        | HermesDesktopError::RemoveProfileMetadata(source)
        | HermesDesktopError::CreateProfileGuard(source)
        | HermesDesktopError::WriteProfileGuard(source)
        | HermesDesktopError::RemoveProfileGuard(source)
        | HermesDesktopError::CreateRecoveryDirectory(source)
        | HermesDesktopError::ProtectRecoveryDirectory(source)
        | HermesDesktopError::QuarantineRecreatedProfile(source)
        | HermesDesktopError::ReadProfiles(source)
        | HermesDesktopError::RemoveProfile(source)
        | HermesDesktopError::ReadProfileConfig(source)
        | HermesDesktopError::CreateBackupDirectory(source)
        | HermesDesktopError::ProtectBackupDirectory(source)
        | HermesDesktopError::ReadBackup(source)
        | HermesDesktopError::Restore(source)
        | HermesDesktopError::RemoveReceipt(source)
        | HermesDesktopError::RemoveBackup(source)
        | HermesDesktopError::ReadFile(source) => {
            io_diagnostic(DiagnosticOperation::WriteConfiguration, source)
        }
        HermesDesktopError::Persistence(_) | HermesDesktopError::Compatibility(_) => {
            Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
        }
        _ => unreachable!("filesystem diagnostic called for another error"),
    }
}
