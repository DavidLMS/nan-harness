//! Payload-free failure boundaries for an explicitly opted-in private hosted trial.

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Persistence,
    RememberedModel,
    Paths,
    SessionLock,
    PendingRecovery,
    ProcessQuery,
    ProcessPresent,
    Credentials,
    Bridge,
    Snapshot,
    ReceiptWrite,
    Configuration,
    VendorLaunch,
}

#[derive(Clone, Copy)]
pub(super) enum ConfigurationSubstage {
    #[cfg(feature = "desktop-qualification")]
    MacPolicy,
    #[cfg(feature = "desktop-qualification")]
    WindowsPolicy,
    #[cfg(feature = "desktop-qualification")]
    LinuxPolicy,
    DocumentRead,
    #[cfg(feature = "desktop-qualification")]
    ManagedMcp,
    Serialize,
    ExistingPermissions,
    ParentCreate,
    PathCheck,
    TemporaryCreate,
    TemporaryWrite,
    TemporaryPermissions,
    Persist,
}

#[derive(Clone, Copy)]
pub(super) enum ConfigurationDocument {
    NormalConfig,
    ThirdPartyConfig,
    Metadata,
    Profile,
}

impl ConfigurationDocument {
    pub(super) fn all() -> [Self; 4] {
        [
            Self::NormalConfig,
            Self::ThirdPartyConfig,
            Self::Metadata,
            Self::Profile,
        ]
    }
}

pub(super) fn observe_configuration<T, E>(
    result: Result<T, E>,
    substage: ConfigurationSubstage,
    document: Option<ConfigurationDocument>,
) -> Result<T, E> {
    if result.is_err() {
        emit_at(Stage::Configuration, Some((substage, document)), None, None);
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigurationIoFailure {
    SharingViolation,
    AccessDenied,
    InvalidName,
    PathNotFound,
    AlreadyExists,
    InvalidInput,
    Other,
}
impl ConfigurationIoFailure {
    pub(super) fn from_error(error: &std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(32) => Self::SharingViolation,
            Some(5) => Self::AccessDenied,
            Some(123) => Self::InvalidName,
            Some(3) => Self::PathNotFound,
            Some(80 | 183) => Self::AlreadyExists,
            Some(87) => Self::InvalidInput,
            _ if error.kind() == std::io::ErrorKind::InvalidInput => Self::InvalidInput,
            _ => Self::Other,
        }
    }
    #[cfg(feature = "desktop-qualification")]
    fn as_str(self) -> &'static str {
        match self {
            Self::SharingViolation => "sharing-violation",
            Self::AccessDenied => "access-denied",
            Self::InvalidName => "invalid-name",
            Self::PathNotFound => "path-not-found",
            Self::AlreadyExists => "already-exists",
            Self::InvalidInput => "invalid-input",
            Self::Other => "other",
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StdRenameBoundary {
    OriginalSource,
    BridgeReader,
    RetainedReader,
    DestinationPreflight,
    RenameDispatch,
    DestinationIdentity,
    PrivatePostcheck,
    Deadline,
}
#[cfg(windows)]
impl StdRenameBoundary {
    fn as_str(self) -> &'static str {
        match self {
            Self::OriginalSource => "original-source",
            Self::BridgeReader => "bridge-reader",
            Self::RetainedReader => "retained-reader",
            Self::DestinationPreflight => "destination-preflight",
            Self::RenameDispatch => "rename-dispatch",
            Self::DestinationIdentity => "destination-identity",
            Self::PrivatePostcheck => "private-postcheck",
            Self::Deadline => "deadline",
        }
    }
}
#[cfg(windows)]
pub(super) fn observe_configuration_rename<T>(
    result: Result<T, super::ClaudeDesktopError>,
    boundary: Option<StdRenameBoundary>,
) -> Result<T, super::ClaudeDesktopError> {
    #[cfg(feature = "desktop-qualification")]
    if let Err(super::ClaudeDesktopError::Write(error)) = &result
        && enabled()
    {
        let mut record = serde_json::json!({"schemaVersion":1,"mechanism":"claude-cli-prelaunch",
            "phase":"prelaunch","stage":"configuration","status":"failed","diagnosticsOnly":true,
            "configurationSubstage":"persist","configurationDocument":"normal-config",
            "configurationIoFailure":ConfigurationIoFailure::from_error(error).as_str(),
            "stdRenameSelected":boundary.is_some()});
        if let Some(boundary) = boundary {
            record["stdRenameBoundary"] = serde_json::json!(boundary.as_str());
        }
        if let Some(directory) = facts_directory() {
            write_record(&directory, &record);
        }
    }
    #[cfg(not(feature = "desktop-qualification"))]
    let _ = boundary;
    result
}

pub(super) fn observe_configuration_persist<T>(
    result: Result<T, super::ClaudeDesktopError>,
    document: Option<ConfigurationDocument>,
) -> Result<T, super::ClaudeDesktopError> {
    if let (Err(error), Some(document)) = (&result, document) {
        let failure = match error {
            super::ClaudeDesktopError::Write(error) => {
                Some(ConfigurationIoFailure::from_error(error))
            }
            _ => None,
        };
        emit_at(
            Stage::Configuration,
            Some((ConfigurationSubstage::Persist, Some(document))),
            failure,
            None,
        );
    }
    result
}

#[cfg(any(windows, test))]
pub(super) fn persist_attributes(before: Option<u32>, after: Option<u32>) -> [Option<bool>; 4] {
    [
        before.map(|bits| bits & 0x100 != 0),
        after.map(|bits| bits & 0x100 != 0),
        before.map(|bits| bits & 1 != 0),
        after.map(|bits| bits & 1 != 0),
    ]
}
#[cfg(any(windows, test))]
pub(super) fn observe_configuration_persist_attributes<T>(
    result: Result<T, super::ClaudeDesktopError>,
    document: Option<ConfigurationDocument>,
    attributes: [Option<bool>; 4],
) -> Result<T, super::ClaudeDesktopError> {
    if let (Err(super::ClaudeDesktopError::Write(error)), Some(ConfigurationDocument::NormalConfig)) =
        (&result, document)
        && error.raw_os_error() == Some(32)
    {
        // Same retained temporary File, sampled once before the first persist and
        // after its final failure. No path reopen, mutation or extra persist attempt.
        emit_at(
            Stage::Configuration,
            Some((ConfigurationSubstage::Persist, document)),
            Some(ConfigurationIoFailure::SharingViolation),
            Some(attributes),
        );
        result
    } else {
        observe_configuration_persist(result, document)
    }
}

pub(super) fn observe<T, E>(result: Result<T, E>, stage: Stage) -> Result<T, E> {
    if result.is_err() {
        emit(stage);
    }
    result
}

fn emit(stage: Stage) {
    emit_at(stage, None, None, None);
}

fn emit_at(
    stage: Stage,
    configuration: Option<(ConfigurationSubstage, Option<ConfigurationDocument>)>,
    io_failure: Option<ConfigurationIoFailure>,
    file_attributes: Option<[Option<bool>; 4]>,
) {
    #[cfg(feature = "desktop-qualification")]
    if enabled() {
        let stage = match stage {
            Stage::Persistence => "persistence",
            Stage::RememberedModel => "remembered-model",
            Stage::Paths => "paths",
            Stage::SessionLock => "session-lock",
            Stage::PendingRecovery => "pending-recovery",
            Stage::ProcessQuery => "process-query",
            Stage::ProcessPresent => "process-present",
            Stage::Credentials => "credentials",
            Stage::Bridge => "bridge",
            Stage::Snapshot => "snapshot",
            Stage::ReceiptWrite => "receipt-write",
            Stage::Configuration => "configuration",
            Stage::VendorLaunch => "vendor-launch",
        };
        let mut record = serde_json::json!({"schemaVersion":1,"mechanism":"claude-cli-prelaunch",
            "phase":"prelaunch","stage":stage,"status":"failed","diagnosticsOnly":true});
        if let Some((substage, document)) = configuration {
            record["configurationSubstage"] = serde_json::json!(substage.as_str());
            if let Some(document) = document {
                record["configurationDocument"] = serde_json::json!(document.as_str());
            }
        }
        if let Some(failure) = io_failure {
            record["configurationIoFailure"] = serde_json::json!(failure.as_str());
        }
        if let Some(
            [
                temporary_before,
                temporary_after,
                readonly_before,
                readonly_after,
            ],
        ) = file_attributes
        {
            record["configurationFileAttributes"] = serde_json::json!({"temporaryBefore":temporary_before,
                "temporaryAfter":temporary_after,"readonlyBefore":readonly_before,"readonlyAfter":readonly_after});
        }
        if let Some(directory) = facts_directory() {
            write_record(&directory, &record);
        }
    }
    #[cfg(not(feature = "desktop-qualification"))]
    let _ = (stage, configuration, io_failure, file_attributes);
}

#[cfg(feature = "desktop-qualification")]
fn write_record(directory: &std::path::Path, record: &serde_json::Value) {
    // The detailed inner boundary wins; outer observations cannot replace it.
    if let Ok(mut file) =
        nan_harness_private_fs::open_private_new(&directory.join("claude-cli-prelaunch.json"))
    {
        let _ = serde_json::to_writer(&mut file, record);
    }
}

#[cfg(feature = "desktop-qualification")]
impl ConfigurationSubstage {
    fn as_str(self) -> &'static str {
        match self {
            Self::MacPolicy => "mac-policy",
            Self::WindowsPolicy => "windows-policy",
            Self::LinuxPolicy => "linux-policy",
            Self::DocumentRead => "document-read",
            Self::ManagedMcp => "managed-mcp",
            Self::Serialize => "serialize",
            Self::ExistingPermissions => "existing-permissions",
            Self::ParentCreate => "parent-create",
            Self::PathCheck => "path-check",
            Self::TemporaryCreate => "temporary-create",
            Self::TemporaryWrite => "temporary-write",
            Self::TemporaryPermissions => "temporary-permissions",
            Self::Persist => "persist",
        }
    }
}
#[cfg(feature = "desktop-qualification")]
impl ConfigurationDocument {
    fn as_str(self) -> &'static str {
        match self {
            Self::NormalConfig => "normal-config",
            Self::ThirdPartyConfig => "third-party-config",
            Self::Metadata => "metadata",
            Self::Profile => "profile",
        }
    }
}

#[cfg(feature = "desktop-qualification")]
pub(super) fn facts_directory() -> Option<std::path::PathBuf> {
    let directory = std::path::PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
    let metadata = std::fs::symlink_metadata(&directory).ok()?;
    if metadata.file_type().is_symlink()
        || !directory.is_absolute()
        || !super::qualification_config::private_directory(&directory)
    {
        return None;
    }
    Some(directory)
}

#[cfg(feature = "desktop-qualification")]
pub(super) fn enabled() -> bool {
    enabled_values(cfg!(windows), |key| std::env::var(key).ok())
}

#[cfg(any(feature = "desktop-qualification", test))]
fn enabled_values(windows: bool, value: impl Fn(&str) -> Option<String>) -> bool {
    windows
        && [
            ("NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS", "1"),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_OS", "Windows"),
            ("NANH_CLAUDE_WINDOWS_PROFILE_POLICY", "private-env"),
            ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
        ]
        .into_iter()
        .all(|(key, expected)| value(key).as_deref() == Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_file_attributes_preserve_unknown_and_separate_bits() {
        assert_eq!(persist_attributes(None, None), [None; 4]);
        assert_eq!(
            persist_attributes(Some(0x80), Some(0x101)),
            [Some(false), Some(true), Some(false), Some(true)]
        );
        assert_eq!(
            persist_attributes(Some(0x100), None),
            [Some(true), None, Some(false), None]
        );
        let result = observe_configuration_persist_attributes::<()>(
            Err(super::super::ClaudeDesktopError::Write(
                std::io::Error::from_raw_os_error(32),
            )),
            None,
            [Some(false); 4],
        );
        let Err(super::super::ClaudeDesktopError::Write(error)) = result else {
            panic!("original error")
        };
        assert_eq!(error.raw_os_error(), Some(32));
        assert_eq!(
            observe_configuration_persist_attributes::<u8>(Ok(7), None, [None; 4]).unwrap(),
            7
        );
    }

    #[test]
    fn windows_persist_error_classification_is_closed_and_preserves_original_error() {
        for (code, expected) in [
            (32, ConfigurationIoFailure::SharingViolation),
            (5, ConfigurationIoFailure::AccessDenied),
            (123, ConfigurationIoFailure::InvalidName),
            (3, ConfigurationIoFailure::PathNotFound),
            (80, ConfigurationIoFailure::AlreadyExists),
            (183, ConfigurationIoFailure::AlreadyExists),
            (87, ConfigurationIoFailure::InvalidInput),
            (987_654, ConfigurationIoFailure::Other),
        ] {
            let error = std::io::Error::from_raw_os_error(code);
            assert_eq!(ConfigurationIoFailure::from_error(&error), expected);
            let result = observe_configuration_persist::<()>(
                Err(super::super::ClaudeDesktopError::Write(error)),
                None,
            );
            let Err(super::super::ClaudeDesktopError::Write(error)) = result else {
                panic!("original write error")
            };
            assert_eq!(error.raw_os_error(), Some(code));
        }
        let error = std::io::Error::other("PRIVATE_SENTINEL");
        assert_eq!(
            ConfigurationIoFailure::from_error(&error),
            ConfigurationIoFailure::Other
        );
        assert_eq!(observe_configuration_persist::<u8>(Ok(7), None).unwrap(), 7);
    }

    #[test]
    fn boundary_preserves_original_error_and_success_values() {
        assert_eq!(
            observe_configuration::<(), _>(
                Err("PRIVATE_SENTINEL"),
                ConfigurationSubstage::Persist,
                Some(ConfigurationDocument::Profile)
            ),
            Err("PRIVATE_SENTINEL")
        );
        assert_eq!(
            observe_configuration::<_, ()>(
                Ok(91),
                ConfigurationSubstage::DocumentRead,
                Some(ConfigurationDocument::NormalConfig)
            ),
            Ok(91)
        );
        assert_eq!(observe::<(), _>(Err(37), Stage::Configuration), Err(37));
        assert_eq!(observe::<_, ()>(Ok(91), Stage::VendorLaunch), Ok(91));
    }
    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn first_configuration_boundary_is_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let inner = serde_json::json!({"stage":"configuration", "configurationSubstage":"persist", "configurationDocument":"profile"});
        write_record(directory.path(), &inner);
        write_record(
            directory.path(),
            &serde_json::json!({"stage":"configuration"}),
        );
        let bytes = std::fs::read(directory.path().join("claude-cli-prelaunch.json")).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            inner
        );
    }

    #[test]
    fn scope_rejects_missing_opt_in_or_foreign_host() {
        let values = |key: &str| {
            Some(
                match key {
                    "NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS" => "1",
                    "GITHUB_ACTIONS" => "true",
                    "RUNNER_ENVIRONMENT" => "github-hosted",
                    "RUNNER_OS" => "Windows",
                    "NANH_CLAUDE_WINDOWS_PROFILE_POLICY" => "private-env",
                    "NANH_DESKTOP_QUALIFICATION_MODE" => "startup-baseline",
                    _ => "",
                }
                .to_owned(),
            )
        };
        assert!(enabled_values(true, values));
        assert!(!enabled_values(false, values));
        assert!(!enabled_values(true, |key| {
            if key == "NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS" {
                None
            } else {
                values(key)
            }
        }));
        assert!(!enabled_values(
            true,
            |key| if key == "RUNNER_ENVIRONMENT" {
                Some("self-hosted".into())
            } else {
                values(key)
            }
        ));
    }
}
