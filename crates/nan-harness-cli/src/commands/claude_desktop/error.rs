use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ClaudeDesktopError {
    #[error("Claude Desktop integration is available only on macOS, Linux, and Windows")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "Claude Desktop is already running; quit it completely, then re-run `nanh claude-desktop`"
    )]
    AlreadyRunning,
    #[error("another `nanh claude-desktop` session is active")]
    ConcurrentSession,
    #[error(
        "an interrupted Claude Desktop session needs recovery; run `nanh claude-desktop --restore`"
    )]
    OrphanReceipt,
    #[error("no interrupted Claude Desktop configuration receipt was found")]
    NoReceipt,
    #[error("Claude Desktop did not start; its original configuration has been restored")]
    DidNotStart,
    #[error(
        "Claude Desktop did not quit, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`"
    )]
    DidNotTerminate,
    #[error(
        "Claude Desktop was not found for {platform}; install the official app from https://support.claude.com/es/articles/10065433-instalar-claude-desktop"
    )]
    AppNotFound { platform: &'static str },
    #[error(transparent)]
    Bridge(#[from] nan_harness_runtime::ClaudeDesktopBridgeError),
    #[error("could not determine the current user's home directory")]
    MissingHome,
    #[error("could not resolve the current user's {0} directory")]
    MissingPlatformDirectory(&'static str),
    #[error("Claude Desktop state path is invalid")]
    InvalidStatePath,
    #[error("Claude Desktop managed state contains an unsafe symbolic link")]
    UnsafeSymlink,
    #[error("could not create a configuration directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("could not protect private Claude Desktop state: {0}")]
    Permissions(std::io::Error),
    #[error("could not lock the Claude Desktop integration: {0}")]
    Lock(std::io::Error),
    #[error("could not check whether Claude Desktop is running: {0}")]
    ProcessCheck(std::io::Error),
    #[error("the Claude Desktop process check failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[error("could not launch Claude Desktop: {0}")]
    Launch(std::io::Error),
    #[error("Claude Desktop launcher failed with exit code {0:?}")]
    LaunchFailed(Option<i32>),
    #[error(
        "could not terminate Claude Desktop, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`: {0}"
    )]
    Terminate(std::io::Error),
    #[error(
        "Claude Desktop termination failed with exit code {0:?}, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`"
    )]
    TerminateFailed(Option<i32>),
    #[error("could not read Claude Desktop configuration: {0}")]
    ReadConfig(std::io::Error),
    #[error("Claude Desktop configuration is not valid JSON: {0}")]
    ParseConfig(serde_json::Error),
    #[error("Claude Desktop configuration root must be an object")]
    ConfigRoot,
    #[error("could not serialize Claude Desktop configuration: {0}")]
    SerializeConfig(serde_json::Error),
    #[error("could not write Claude Desktop configuration: {0}")]
    Write(std::io::Error),
    #[error("could not restore Claude Desktop configuration: {0}")]
    Restore(std::io::Error),
    #[error(
        "an orphaned Claude Desktop backup exists; inspect the private state directory before retrying"
    )]
    OrphanBackup,
    #[error("could not create the private Claude Desktop backup directory: {0}")]
    CreateBackupDirectory(std::io::Error),
    #[error("could not write a private Claude Desktop backup: {0}")]
    WriteBackup(std::io::Error),
    #[error("could not read a private Claude Desktop backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Claude Desktop backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Claude Desktop backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not serialize the private Claude Desktop receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("could not read the private Claude Desktop receipt: {0}")]
    ReadReceipt(std::io::Error),
    #[error("the private Claude Desktop receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the private Claude Desktop receipt schema is not supported")]
    UnsupportedReceipt,
    #[error("could not remove the restored Claude Desktop receipt: {0}")]
    RemoveReceipt(std::io::Error),
}

impl ClaudeDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(error) => error.code(),
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::OrphanReceipt
            | Self::OrphanBackup
            | Self::UnsafeSymlink => "NH-DESKTOP-002",
            Self::UnsupportedPlatform
            | Self::AppNotFound { .. }
            | Self::Compatibility(
                nan_harness_runtime::DesktopCompatibilityError::Unavailable
                | nan_harness_runtime::DesktopCompatibilityError::MissingPlatform,
            ) => "NH-DESKTOP-003",
            _ => "NH-DESKTOP-001",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudeDesktopError;

    #[test]
    fn command_recovery_messages_keep_the_nanh_alias() {
        assert_eq!(
            ClaudeDesktopError::AlreadyRunning.to_string(),
            "Claude Desktop is already running; quit it completely, then re-run `nanh claude-desktop`"
        );
        assert_eq!(
            ClaudeDesktopError::OrphanReceipt.to_string(),
            "an interrupted Claude Desktop session needs recovery; run `nanh claude-desktop --restore`"
        );
        assert_eq!(
            ClaudeDesktopError::DidNotTerminate.to_string(),
            "Claude Desktop did not quit, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`"
        );
    }

    #[test]
    fn extracted_errors_keep_their_diagnostic_codes() {
        assert_eq!(
            ClaudeDesktopError::UnsupportedPlatform.code(),
            "NH-DESKTOP-003"
        );
        assert_eq!(
            ClaudeDesktopError::ConcurrentSession.code(),
            "NH-DESKTOP-002"
        );
        assert_eq!(ClaudeDesktopError::DidNotStart.code(), "NH-DESKTOP-001");
    }
}
