use super::{Classification, io};
use crate::commands::install::InstallError;
use nan_harness_runtime::DiscoveryError;
use nan_harness_telemetry::event::FailureCause;

pub(super) fn classify_discovery(error: &DiscoveryError) -> Classification {
    match error {
        DiscoveryError::ExecutableNotFound(_) => (FailureCause::MissingExecutable, None),
        DiscoveryError::InvalidExecutable(_) => (FailureCause::PermissionDenied, None),
        DiscoveryError::VersionCommand { source, .. } => (io::classify(source), None),
        DiscoveryError::VersionCommandFailed { .. } => (FailureCause::ProcessExit, None),
        DiscoveryError::UnsupportedVersion { .. } | DiscoveryError::UnparseableVersion { .. } => {
            (FailureCause::UnsupportedVersion, None)
        }
        DiscoveryError::InvalidManifest(_)
        | DiscoveryError::InvalidManifestContract(_)
        | DiscoveryError::MissingCompatibilityEntry(_)
        | DiscoveryError::InvalidVersionCommand { .. } => (FailureCause::InvalidData, None),
    }
}

pub(super) fn classify_install(error: &InstallError) -> Classification {
    match error {
        InstallError::Prompt(source)
        | InstallError::DownloadStart { source, .. }
        | InstallError::PrepareInstaller { source, .. }
        | InstallError::InstallerStart { source, .. }
        | InstallError::CommandStart { source, .. }
        | InstallError::RuntimeCommandStart { source, .. }
        | InstallError::PostInstallCheckStart { source, .. }
        | InstallError::PostInstallCheckPrepare { source, .. } => (io::classify(source), None),
        InstallError::DownloadFailed { .. }
        | InstallError::InstallerFailed { .. }
        | InstallError::CommandFailed { .. }
        | InstallError::RuntimeCommandFailed { .. }
        | InstallError::PostInstallCheckFailed { .. } => (FailureCause::ProcessExit, None),
        InstallError::RuntimeUnsupported { .. } | InstallError::RuntimeUnparseable { .. } => {
            (FailureCause::UnsupportedVersion, None)
        }
        InstallError::CompatibilityManifest(_)
        | InstallError::InvalidRuntimeCommand { .. }
        | InstallError::UnsupportedPlatform(_)
        | InstallError::UnsupportedHarness(_) => (FailureCause::InvalidConfiguration, None),
    }
}
