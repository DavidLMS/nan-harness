use super::{Classification, io};
use crate::commands::install::InstallError;
use nan_harness_runtime::DiscoveryError;
use nan_harness_telemetry::event::FailureCause;

pub(super) fn classify_discovery(error: &DiscoveryError) -> Classification {
    match error {
        DiscoveryError::VersionProbeTimeout => (FailureCause::Timeout, None),
        DiscoveryError::ExecutableNotFound(_) => (FailureCause::MissingExecutable, None),
        DiscoveryError::InvalidExecutable(_) => (FailureCause::PermissionDenied, None),
        DiscoveryError::VersionCommand { source, .. } => (io::classify(source), None),
        DiscoveryError::VersionCommandFailed { .. } => (FailureCause::ProcessExit, None),
        DiscoveryError::UnsupportedVersion { .. } | DiscoveryError::UnparseableVersion { .. } => {
            (FailureCause::UnsupportedVersion, None)
        }
        DiscoveryError::VersionProbeOutputLimit
        | DiscoveryError::InvalidManifest(_)
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

#[cfg(test)]
mod probe_tests {
    use super::{DiscoveryError, FailureCause, classify_discovery};

    #[test]
    fn probe_failures_classify_timeout_and_output_separately() {
        assert_eq!(
            classify_discovery(&DiscoveryError::VersionProbeTimeout),
            (FailureCause::Timeout, None)
        );
        assert_eq!(
            classify_discovery(&DiscoveryError::VersionProbeOutputLimit),
            (FailureCause::InvalidData, None)
        );
    }
}
