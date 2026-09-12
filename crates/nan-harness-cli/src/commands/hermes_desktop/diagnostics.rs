#[allow(clippy::wildcard_imports)]
use super::*;

mod filesystem;
pub(super) use filesystem::filesystem_diagnostic;

pub(super) fn append_diagnostics(
    target: &mut Vec<BridgeDiagnostic>,
    diagnostics: Vec<BridgeDiagnostic>,
) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum HermesDesktopError {
    #[error("Hermes Desktop is already open; close it before running `nanh hermes-desktop`")]
    AlreadyRunning,
    #[error("another `nanh hermes-desktop` session is active")]
    ConcurrentSession,
    #[error("Hermes Desktop is updating; wait for it to finish before retrying")]
    UpdateAlreadyRunning,
    #[error(
        "Hermes Desktop's updater is still running; wait for it to finish, then run `nanh hermes-desktop --restore`"
    )]
    UpdateStillRunning,
    #[error(
        "Hermes Desktop's update exceeded 20 minutes; wait for it to finish, then run `nanh hermes-desktop --restore`"
    )]
    UpdateTimedOut,
    #[error(
        "Hermes Desktop completed its update but did not relaunch; start it again with `nanh hermes-desktop`"
    )]
    DidNotRelaunch,
    #[error(
        "the Hermes profile 'nan' already exists and is not managed by nan-harness; rename that profile before running `nanh hermes-desktop`"
    )]
    UnmanagedNanProfile,
    #[error(
        "both active and parked managed Hermes profiles exist; preserve both directories and resolve the duplicate before retrying"
    )]
    ManagedProfileConflict,
    #[error(
        "the parked Hermes profile does not have matching nan-harness ownership; move it aside before retrying"
    )]
    ParkedProfileOwnershipMismatch,
    #[error(
        "the Hermes profile visibility guard does not match nan-harness ownership; preserve that entry and run `nanh hermes-desktop --restore`"
    )]
    ProfileGuardOwnershipMismatch,
    #[error("the nan-harness ownership receipt exists but its Hermes profile is missing")]
    ManagedProfileMissing,
    #[error("the Hermes profile ownership marker does not match nan-harness state")]
    OwnershipMismatch,
    #[error("the Hermes Desktop ownership receipt schema is not supported")]
    UnsupportedOwnershipSchema,
    #[error(
        "a previous Hermes Desktop session needs recovery; run `nanh hermes-desktop --restore`"
    )]
    PendingRecovery,
    #[error("--restore cannot be combined with launch options")]
    RestoreWithLaunchOptions,
    #[error("Hermes Desktop argument '{0}' is incompatible with a managed NaN launch")]
    UnsupportedDesktopArgument(&'static str),
    #[error(
        "Hermes Desktop requires Hermes Agent {minimum} or newer; found {detected}; update Hermes or pass --allow-unsupported"
    )]
    DesktopVersionUnsupported { detected: Version, minimum: Version },
    #[error("Hermes Desktop is unavailable on this platform")]
    DesktopUnavailable,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error("the embedded Hermes Desktop compatibility evidence is incomplete")]
    InvalidCompatibilityEvidence,
    #[error("could not inspect Hermes Desktop launch capabilities: {0}")]
    CapabilityProbe(std::io::Error),
    #[error("Hermes Desktop capability probe failed with exit code {0:?}")]
    CapabilityProbeFailed(Option<i32>),
    #[error("Hermes Desktop is missing required launch capabilities: {0}")]
    MissingDesktopCapabilities(String),
    #[error(
        "model '{model}' is not available for this NaN credential; choose one of: {available:?}"
    )]
    ModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("NaN returned no conversational models")]
    EmptyModelCatalog,
    #[error("the configured stable Hermes Desktop gateway port {port} is unavailable: {source}")]
    StablePortUnavailable { port: u16, source: std::io::Error },
    #[error("could not bind a local Hermes Desktop gateway: {0}")]
    BindGateway(std::io::Error),
    #[error(transparent)]
    Gateway(#[from] ChatGatewayError),
    #[error("the Hermes Desktop gateway stopped unexpectedly")]
    GatewayExited,
    #[error("could not launch Hermes Desktop: {0}")]
    Launch(std::io::Error),
    #[error("could not wait for Hermes Desktop: {0}")]
    Wait(std::io::Error),
    #[error("could not inspect Hermes Desktop processes: {0}")]
    ProcessCheck(std::io::Error),
    #[error("Hermes Desktop process inspection failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[cfg(any(windows, test))]
    #[error("Hermes Desktop process inspection returned invalid JSON: {0}")]
    ParseProcessListing(serde_json::Error),
    #[cfg(any(windows, test))]
    #[error("Hermes Desktop process inspection omitted its process ID")]
    InvalidProcessListing,
    #[cfg(any(windows, test))]
    #[error("multiple Hermes Desktop main processes are running; close them before retrying")]
    AmbiguousDesktopProcesses,
    #[error("could not terminate Hermes Desktop: {0}")]
    Terminate(std::io::Error),
    #[error("Hermes Desktop termination failed with exit code {0:?}")]
    TerminateFailed(Option<i32>),
    #[error("Hermes Desktop did not terminate")]
    DidNotTerminate,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("NAN_HARNESS_CONFIG_DIR must be an absolute path for Hermes Desktop recovery")]
    InvalidStateDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("HERMES_HOME must be an absolute path for Hermes Desktop recovery")]
    InvalidHermesHome,
    #[error("could not create private Hermes Desktop state: {0}")]
    CreateStateDirectory(std::io::Error),
    #[error("could not protect private Hermes Desktop state: {0}")]
    ProtectStateDirectory(std::io::Error),
    #[error("could not open the Hermes Desktop session lock: {0}")]
    OpenLock(std::io::Error),
    #[error("could not lock the Hermes Desktop session: {0}")]
    Lock(std::io::Error),
    #[error("could not create the managed Hermes profile: {0}")]
    CreateProfile(std::io::Error),
    #[error("could not protect the managed Hermes profile: {0}")]
    ProtectProfile(std::io::Error),
    #[error("could not create the private parked-profile directory: {0}")]
    CreateParkingDirectory(std::io::Error),
    #[error("could not protect the private parked-profile directory: {0}")]
    ProtectParkingDirectory(std::io::Error),
    #[error("could not activate the managed Hermes profile: {0}")]
    ActivateProfile(std::io::Error),
    #[error("could not park the managed Hermes profile: {0}")]
    ParkProfile(std::io::Error),
    #[error("could not remove legacy Hermes profile metadata: {0}")]
    RemoveProfileMetadata(std::io::Error),
    #[error("could not create the Hermes profile visibility guard: {0}")]
    CreateProfileGuard(std::io::Error),
    #[error("could not write the Hermes profile visibility guard: {0}")]
    WriteProfileGuard(std::io::Error),
    #[error("could not remove the Hermes profile visibility guard: {0}")]
    RemoveProfileGuard(std::io::Error),
    #[error("could not create the private recreated-profile recovery directory: {0}")]
    CreateRecoveryDirectory(std::io::Error),
    #[error("could not protect the private recreated-profile recovery directory: {0}")]
    ProtectRecoveryDirectory(std::io::Error),
    #[error("could not preserve the recreated Hermes profile for recovery: {0}")]
    QuarantineRecreatedProfile(std::io::Error),
    #[error("could not enumerate Hermes profiles: {0}")]
    ReadProfiles(std::io::Error),
    #[error("could not remove an owned diagnostic Hermes profile: {0}")]
    RemoveProfile(std::io::Error),
    #[error("a diagnostic Hermes profile no longer has its nan-harness ownership marker")]
    DiagnosticOwnershipMismatch,
    #[error("could not read the managed Hermes profile configuration: {0}")]
    ReadProfileConfig(std::io::Error),
    #[error("the managed Hermes profile uses an unsupported YAML form for '{0}'")]
    UnsupportedProfileConfig(String),
    #[error("the managed Hermes profile configuration is invalid YAML: {0}")]
    ParseProfileConfig(serde_yaml_ng::Error),
    #[error("could not serialize the managed Hermes profile configuration: {0}")]
    SerializeProfileConfig(serde_yaml_ng::Error),
    #[error("the managed Hermes profile contains an unsafe symbolic link")]
    UnsafePluginPath,
    #[error("the shared Hermes search renderer did not provide config.yaml")]
    MissingSearchTemplate,
    #[error(
        "the managed Hermes profile already defines NAN_API_KEY; remove that entry before running `nanh hermes-desktop`"
    )]
    ProfileCredentialConflict,
    #[error(
        "the managed Hermes profile credential block changed; remove the nan-harness NAN_API_KEY block, then run `nanh hermes-desktop --restore`"
    )]
    ManagedCredentialChanged,
    #[error("the managed Hermes profile .env is not UTF-8: {0}")]
    ProfileEnvUtf8(std::str::Utf8Error),
    #[error("the managed Hermes profile path is invalid")]
    InvalidProfilePath,
    #[error("could not create private Hermes Desktop recovery backups: {0}")]
    CreateBackupDirectory(std::io::Error),
    #[error("could not protect private Hermes Desktop recovery backups: {0}")]
    ProtectBackupDirectory(std::io::Error),
    #[error("could not read a private Hermes Desktop recovery backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Hermes Desktop recovery backup does not match its receipt")]
    BackupHashMismatch,
    #[error("could not restore Hermes Desktop state: {0}")]
    Restore(std::io::Error),
    #[error("could not remove the Hermes Desktop recovery receipt: {0}")]
    RemoveReceipt(std::io::Error),
    #[error("could not remove private Hermes Desktop recovery backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not read Hermes Desktop's update marker: {0}")]
    ReadUpdateMarker(std::io::Error),
    #[error("could not read Hermes Desktop managed state: {0}")]
    ReadFile(std::io::Error),
    #[error("Hermes Desktop managed state is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the Hermes Desktop recovery receipt schema is not supported")]
    UnsupportedSessionSchema,
    #[error("the Hermes Desktop recovery receipt contains an unsafe path")]
    InvalidRecoveryReceipt,
    #[error("could not serialize Hermes Desktop managed state: {0}")]
    Serialize(serde_json::Error),
    #[error("could not generate a private Hermes Desktop identifier: {0}")]
    Random(getrandom::Error),
    #[error("could not access the NaN credential: {0}")]
    Secret(nan_harness_core::SecretError),
    #[error(transparent)]
    Persistence(crate::commands::persistence::PersistenceError),
}

impl HermesDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::UpdateAlreadyRunning
            | Self::UnmanagedNanProfile
            | Self::ManagedProfileConflict
            | Self::ParkedProfileOwnershipMismatch
            | Self::ProfileGuardOwnershipMismatch
            | Self::ManagedProfileMissing
            | Self::OwnershipMismatch
            | Self::PendingRecovery
            | Self::RestoreWithLaunchOptions
            | Self::UnsupportedDesktopArgument(_)
            | Self::DesktopVersionUnsupported { .. }
            | Self::DesktopUnavailable
            | Self::MissingDesktopCapabilities(_)
            | Self::InvalidStateDirectory
            | Self::InvalidHermesHome
            | Self::ProfileCredentialConflict
            | Self::ManagedCredentialChanged
            | Self::DiagnosticOwnershipMismatch => "NH-HERMES-DESKTOP-002",
            Self::ModelUnavailable { .. } | Self::EmptyModelCatalog => "NH-HERMES-DESKTOP-003",
            Self::UpdateStillRunning
            | Self::UpdateTimedOut
            | Self::DidNotRelaunch
            | Self::ReadUpdateMarker(_) => "NH-HERMES-DESKTOP-004",
            _ => "NH-HERMES-DESKTOP-001",
        }
    }

    // Keep this match exhaustive so every new recovery error receives a typed diagnostic.
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::UpdateAlreadyRunning
            | Self::UnmanagedNanProfile
            | Self::ManagedProfileConflict
            | Self::ParkedProfileOwnershipMismatch
            | Self::ProfileGuardOwnershipMismatch
            | Self::ManagedProfileMissing
            | Self::OwnershipMismatch
            | Self::PendingRecovery
            | Self::RestoreWithLaunchOptions
            | Self::UnsupportedDesktopArgument(_)
            | Self::ProfileCredentialConflict
            | Self::ManagedCredentialChanged
            | Self::DiagnosticOwnershipMismatch
            | Self::UnsafePluginPath => configuration_conflict_diagnostic(),
            Self::ModelUnavailable { .. } | Self::EmptyModelCatalog => {
                model_catalog_diagnostic(self)
            }
            Self::Gateway(error) => gateway_diagnostic(error),
            Self::Secret(_) | Self::Random(_) | Self::GatewayExited => {
                gateway_support_diagnostic(self)
            }
            Self::StablePortUnavailable { .. }
            | Self::BindGateway(_)
            | Self::Launch(_)
            | Self::CapabilityProbe(_)
            | Self::Wait(_)
            | Self::ProcessCheck(_)
            | Self::Terminate(_) => process_io_diagnostic(self),
            Self::UpdateStillRunning
            | Self::UpdateTimedOut
            | Self::DidNotRelaunch
            | Self::ReadUpdateMarker(_)
            | Self::CapabilityProbeFailed(_)
            | Self::ProcessCheckFailed(_)
            | Self::TerminateFailed(_)
            | Self::DidNotTerminate => process_wait_diagnostic(),
            #[cfg(any(windows, test))]
            Self::ParseProcessListing(_)
            | Self::InvalidProcessListing
            | Self::AmbiguousDesktopProcesses => invalid_process_listing_diagnostic(),
            Self::DesktopVersionUnsupported { .. }
            | Self::DesktopUnavailable
            | Self::UnsupportedOwnershipSchema
            | Self::UnsupportedSessionSchema => version_diagnostic(),
            Self::UnsupportedProfileConfig(_)
            | Self::ParseProfileConfig(_)
            | Self::SerializeProfileConfig(_)
            | Self::InvalidCompatibilityEvidence
            | Self::MissingSearchTemplate
            | Self::MissingDesktopCapabilities(_)
            | Self::ProfileEnvUtf8(_)
            | Self::InvalidProfilePath
            | Self::InvalidStateDirectory
            | Self::InvalidHermesHome
            | Self::ParseReceipt(_)
            | Self::InvalidRecoveryReceipt
            | Self::BackupHashMismatch => invalid_configuration_diagnostic(),
            Self::Serialize(_)
            | Self::MissingStateDirectory
            | Self::MissingHomeDirectory
            | Self::CreateStateDirectory(_)
            | Self::ProtectStateDirectory(_)
            | Self::OpenLock(_)
            | Self::Lock(_)
            | Self::CreateProfile(_)
            | Self::ProtectProfile(_)
            | Self::CreateParkingDirectory(_)
            | Self::ProtectParkingDirectory(_)
            | Self::ActivateProfile(_)
            | Self::ParkProfile(_)
            | Self::RemoveProfileMetadata(_)
            | Self::CreateProfileGuard(_)
            | Self::WriteProfileGuard(_)
            | Self::RemoveProfileGuard(_)
            | Self::CreateRecoveryDirectory(_)
            | Self::ProtectRecoveryDirectory(_)
            | Self::QuarantineRecreatedProfile(_)
            | Self::ReadProfiles(_)
            | Self::RemoveProfile(_)
            | Self::ReadProfileConfig(_)
            | Self::CreateBackupDirectory(_)
            | Self::ProtectBackupDirectory(_)
            | Self::ReadBackup(_)
            | Self::Restore(_)
            | Self::RemoveReceipt(_)
            | Self::RemoveBackup(_)
            | Self::ReadFile(_)
            | Self::Persistence(_)
            | Self::Compatibility(_) => filesystem_diagnostic(self),
        }
    }
}

pub(super) fn configuration_conflict_diagnostic() -> Diagnostic {
    Diagnostic::general(DiagnosticReason::ConfigurationConflict)
}

pub(super) fn model_catalog_diagnostic(error: &HermesDesktopError) -> Diagnostic {
    match error {
        HermesDesktopError::ModelUnavailable { .. } => {
            Diagnostic::general(DiagnosticReason::ModelUnavailable)
        }
        HermesDesktopError::EmptyModelCatalog => {
            Diagnostic::general(DiagnosticReason::ModelCatalogEmpty)
        }
        _ => unreachable!("model catalog diagnostic called for another error"),
    }
}

pub(super) fn gateway_support_diagnostic(error: &HermesDesktopError) -> Diagnostic {
    match error {
        HermesDesktopError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        HermesDesktopError::Random(_) => {
            Diagnostic::general(DiagnosticReason::RandomGenerationFailed)
        }
        HermesDesktopError::GatewayExited => Diagnostic::general(DiagnosticReason::BridgeExited),
        _ => unreachable!("gateway support diagnostic called for another error"),
    }
}

pub(super) fn process_io_diagnostic(error: &HermesDesktopError) -> Diagnostic {
    match error {
        HermesDesktopError::StablePortUnavailable { source, .. }
        | HermesDesktopError::BindGateway(source) => {
            io_diagnostic(DiagnosticOperation::BindBridge, source)
        }
        HermesDesktopError::Launch(source) => {
            io_diagnostic(DiagnosticOperation::StartHarness, source)
        }
        HermesDesktopError::CapabilityProbe(source) => {
            io_diagnostic(DiagnosticOperation::RunVersionCommand, source)
        }
        HermesDesktopError::Wait(source) | HermesDesktopError::ProcessCheck(source) => {
            io_diagnostic(DiagnosticOperation::WaitForHarness, source)
        }
        HermesDesktopError::Terminate(source) => {
            io_diagnostic(DiagnosticOperation::StopHarness, source)
        }
        _ => unreachable!("process IO diagnostic called for another error"),
    }
}

pub(super) fn process_wait_diagnostic() -> Diagnostic {
    Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
}

#[cfg(any(windows, test))]
pub(super) fn invalid_process_listing_diagnostic() -> Diagnostic {
    Diagnostic::general(DiagnosticReason::InvalidResponse)
}

pub(super) fn version_diagnostic() -> Diagnostic {
    Diagnostic::general(DiagnosticReason::UnsupportedVersion)
}

pub(super) fn invalid_configuration_diagnostic() -> Diagnostic {
    Diagnostic::general(DiagnosticReason::InvalidConfiguration)
}

pub(super) fn gateway_diagnostic(error: &ChatGatewayError) -> Diagnostic {
    match error {
        ChatGatewayError::Bridge(nan_harness_runtime::BridgeError::SelectedModelUnavailable {
            ..
        }) => Diagnostic::general(DiagnosticReason::ModelUnavailable),
        ChatGatewayError::Bridge(nan_harness_runtime::BridgeError::NoCompatibleModels) => {
            Diagnostic::general(DiagnosticReason::ModelCatalogEmpty)
        }
        ChatGatewayError::Bridge(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
        ChatGatewayError::Secret(_) => {
            Diagnostic::general(DiagnosticReason::SecretResolutionFailed)
        }
        ChatGatewayError::Random(_) => {
            Diagnostic::general(DiagnosticReason::RandomGenerationFailed)
        }
    }
}

pub(super) fn io_diagnostic(operation: DiagnosticOperation, source: &std::io::Error) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: IoErrorKind::from_std(source.kind()),
        },
    )
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for HermesDesktopError {
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive terminal projection keeps every error variant visible"
    )]
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::AlreadyRunning => m::error_hermes_desktop_already_running(locale),
            Self::ConcurrentSession => m::error_hermes_desktop_concurrent_session(locale),
            Self::UpdateAlreadyRunning => m::error_hermes_desktop_update_already_running(locale),
            Self::UpdateStillRunning => m::error_hermes_desktop_update_still_running(locale),
            Self::UpdateTimedOut => m::error_hermes_desktop_update_timed_out(locale),
            Self::DidNotRelaunch => m::error_hermes_desktop_did_not_relaunch(locale),
            Self::UnmanagedNanProfile => m::error_hermes_desktop_unmanaged_nan_profile(locale),
            Self::ManagedProfileConflict => {
                m::error_hermes_desktop_managed_profile_conflict(locale)
            }
            Self::ParkedProfileOwnershipMismatch => {
                m::error_hermes_desktop_parked_profile_ownership_mismatch(locale)
            }
            Self::ProfileGuardOwnershipMismatch => {
                m::error_hermes_desktop_profile_guard_ownership_mismatch(locale)
            }
            Self::ManagedProfileMissing => m::error_hermes_desktop_managed_profile_missing(locale),
            Self::OwnershipMismatch => m::error_hermes_desktop_ownership_mismatch(locale),
            Self::UnsupportedOwnershipSchema => {
                m::error_hermes_desktop_unsupported_ownership_schema(locale)
            }
            Self::PendingRecovery => m::error_hermes_desktop_pending_recovery(locale),
            Self::RestoreWithLaunchOptions => {
                m::error_hermes_desktop_restore_with_launch_options(locale)
            }
            Self::UnsupportedDesktopArgument(field_0) => {
                m::error_hermes_desktop_unsupported_desktop_argument(locale, &(field_0))
            }
            Self::DesktopVersionUnsupported { detected, minimum } => {
                m::error_hermes_desktop_desktop_version_unsupported(locale, &(detected), &(minimum))
            }
            Self::DesktopUnavailable => m::error_hermes_desktop_desktop_unavailable(locale),
            Self::Compatibility(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::InvalidCompatibilityEvidence => {
                m::error_hermes_desktop_invalid_compatibility_evidence(locale)
            }
            Self::CapabilityProbe(field_0) => {
                m::error_hermes_desktop_capability_probe(locale, &(field_0))
            }
            Self::CapabilityProbeFailed(field_0) => {
                m::error_hermes_desktop_capability_probe_failed(locale, &(format!("{field_0:?}")))
            }
            Self::MissingDesktopCapabilities(field_0) => {
                m::error_hermes_desktop_missing_desktop_capabilities(locale, &(field_0))
            }
            Self::ModelUnavailable { model, available } => {
                m::error_hermes_desktop_model_unavailable(
                    locale,
                    &(format!("{available:?}")),
                    &(model),
                )
            }
            Self::EmptyModelCatalog => m::error_hermes_desktop_empty_model_catalog(locale),
            Self::StablePortUnavailable { port, source } => {
                m::error_hermes_desktop_stable_port_unavailable(locale, &(port), &(source))
            }
            Self::BindGateway(field_0) => m::error_hermes_desktop_bind_gateway(locale, &(field_0)),
            Self::Gateway(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::GatewayExited => m::error_hermes_desktop_gateway_exited(locale),
            Self::Launch(field_0) => m::error_hermes_desktop_launch(locale, &(field_0)),
            Self::Wait(field_0) => m::error_hermes_desktop_wait(locale, &(field_0)),
            Self::ProcessCheck(field_0) => {
                m::error_hermes_desktop_process_check(locale, &(field_0))
            }
            Self::ProcessCheckFailed(field_0) => {
                m::error_hermes_desktop_process_check_failed(locale, &(format!("{field_0:?}")))
            }
            #[cfg(any(windows, test))]
            Self::ParseProcessListing(field_0) => {
                m::error_hermes_desktop_parse_process_listing(locale, &(field_0))
            }
            #[cfg(any(windows, test))]
            Self::InvalidProcessListing => m::error_hermes_desktop_invalid_process_listing(locale),
            #[cfg(any(windows, test))]
            Self::AmbiguousDesktopProcesses => {
                m::error_hermes_desktop_ambiguous_desktop_processes(locale)
            }
            Self::Terminate(field_0) => m::error_hermes_desktop_terminate(locale, &(field_0)),
            Self::TerminateFailed(field_0) => {
                m::error_hermes_desktop_terminate_failed(locale, &(format!("{field_0:?}")))
            }
            Self::DidNotTerminate => m::error_hermes_desktop_did_not_terminate(locale),
            Self::MissingStateDirectory => m::error_hermes_desktop_missing_state_directory(locale),
            Self::InvalidStateDirectory => m::error_hermes_desktop_invalid_state_directory(locale),
            Self::MissingHomeDirectory => m::error_hermes_desktop_missing_home_directory(locale),
            Self::InvalidHermesHome => m::error_hermes_desktop_invalid_hermes_home(locale),
            Self::CreateStateDirectory(field_0) => {
                m::error_hermes_desktop_create_state_directory(locale, &(field_0))
            }
            Self::ProtectStateDirectory(field_0) => {
                m::error_hermes_desktop_protect_state_directory(locale, &(field_0))
            }
            Self::OpenLock(field_0) => m::error_hermes_desktop_open_lock(locale, &(field_0)),
            Self::Lock(field_0) => m::error_hermes_desktop_lock(locale, &(field_0)),
            Self::CreateProfile(field_0) => {
                m::error_hermes_desktop_create_profile(locale, &(field_0))
            }
            Self::ProtectProfile(field_0) => {
                m::error_hermes_desktop_protect_profile(locale, &(field_0))
            }
            Self::CreateParkingDirectory(field_0) => {
                m::error_hermes_desktop_create_parking_directory(locale, &(field_0))
            }
            Self::ProtectParkingDirectory(field_0) => {
                m::error_hermes_desktop_protect_parking_directory(locale, &(field_0))
            }
            Self::ActivateProfile(field_0) => {
                m::error_hermes_desktop_activate_profile(locale, &(field_0))
            }
            Self::ParkProfile(field_0) => m::error_hermes_desktop_park_profile(locale, &(field_0)),
            Self::RemoveProfileMetadata(field_0) => {
                m::error_hermes_desktop_remove_profile_metadata(locale, &(field_0))
            }
            Self::CreateProfileGuard(field_0) => {
                m::error_hermes_desktop_create_profile_guard(locale, &(field_0))
            }
            Self::WriteProfileGuard(field_0) => {
                m::error_hermes_desktop_write_profile_guard(locale, &(field_0))
            }
            Self::RemoveProfileGuard(field_0) => {
                m::error_hermes_desktop_remove_profile_guard(locale, &(field_0))
            }
            Self::CreateRecoveryDirectory(field_0) => {
                m::error_hermes_desktop_create_recovery_directory(locale, &(field_0))
            }
            Self::ProtectRecoveryDirectory(field_0) => {
                m::error_hermes_desktop_protect_recovery_directory(locale, &(field_0))
            }
            Self::QuarantineRecreatedProfile(field_0) => {
                m::error_hermes_desktop_quarantine_recreated_profile(locale, &(field_0))
            }
            Self::ReadProfiles(field_0) => {
                m::error_hermes_desktop_read_profiles(locale, &(field_0))
            }
            Self::RemoveProfile(field_0) => {
                m::error_hermes_desktop_remove_profile(locale, &(field_0))
            }
            Self::DiagnosticOwnershipMismatch => {
                m::error_hermes_desktop_diagnostic_ownership_mismatch(locale)
            }
            Self::ReadProfileConfig(field_0) => {
                m::error_hermes_desktop_read_profile_config(locale, &(field_0))
            }
            Self::UnsupportedProfileConfig(field_0) => {
                m::error_hermes_desktop_unsupported_profile_config(locale, &(field_0))
            }
            Self::ParseProfileConfig(field_0) => {
                m::error_hermes_desktop_parse_profile_config(locale, &(field_0))
            }
            Self::SerializeProfileConfig(field_0) => {
                m::error_hermes_desktop_serialize_profile_config(locale, &(field_0))
            }
            Self::UnsafePluginPath => m::error_hermes_desktop_unsafe_plugin_path(locale),
            Self::MissingSearchTemplate => m::error_hermes_desktop_missing_search_template(locale),
            Self::ProfileCredentialConflict => {
                m::error_hermes_desktop_profile_credential_conflict(locale)
            }
            Self::ManagedCredentialChanged => {
                m::error_hermes_desktop_managed_credential_changed(locale)
            }
            Self::ProfileEnvUtf8(field_0) => {
                m::error_hermes_desktop_profile_env_utf8(locale, &(field_0))
            }
            Self::InvalidProfilePath => m::error_hermes_desktop_invalid_profile_path(locale),
            Self::CreateBackupDirectory(field_0) => {
                m::error_hermes_desktop_create_backup_directory(locale, &(field_0))
            }
            Self::ProtectBackupDirectory(field_0) => {
                m::error_hermes_desktop_protect_backup_directory(locale, &(field_0))
            }
            Self::ReadBackup(field_0) => m::error_hermes_desktop_read_backup(locale, &(field_0)),
            Self::BackupHashMismatch => m::error_hermes_desktop_backup_hash_mismatch(locale),
            Self::Restore(field_0) => m::error_hermes_desktop_restore(locale, &(field_0)),
            Self::RemoveReceipt(field_0) => {
                m::error_hermes_desktop_remove_receipt(locale, &(field_0))
            }
            Self::RemoveBackup(field_0) => {
                m::error_hermes_desktop_remove_backup(locale, &(field_0))
            }
            Self::ReadUpdateMarker(field_0) => {
                m::error_hermes_desktop_read_update_marker(locale, &(field_0))
            }
            Self::ReadFile(field_0) => m::error_hermes_desktop_read_file(locale, &(field_0)),
            Self::ParseReceipt(field_0) => {
                m::error_hermes_desktop_parse_receipt(locale, &(field_0))
            }
            Self::UnsupportedSessionSchema => {
                m::error_hermes_desktop_unsupported_session_schema(locale)
            }
            Self::InvalidRecoveryReceipt => {
                m::error_hermes_desktop_invalid_recovery_receipt(locale)
            }
            Self::Serialize(field_0) => m::error_hermes_desktop_serialize(locale, &(field_0)),
            Self::Random(field_0) => m::error_hermes_desktop_random(locale, &(field_0)),
            Self::Secret(field_0) => m::error_hermes_desktop_secret(
                locale,
                &(nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)),
            ),
            Self::Persistence(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
        }
    }
}
