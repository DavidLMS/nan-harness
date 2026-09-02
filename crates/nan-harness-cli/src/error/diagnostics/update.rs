use super::details;
use nan_harness_runtime::update::UpdateError;
use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, DocumentKind, IoErrorKind,
};

pub(super) fn typed(error: &UpdateError) -> Diagnostic {
    match error {
        error @ (UpdateError::UpdateChannelUnavailable
        | UpdateError::MissingConfigDirectory
        | UpdateError::Version(_)
        | UpdateError::InvalidUrl { .. }
        | UpdateError::InsecureUrl(_)) => configuration(error),
        error @ (UpdateError::BuildClient(_)
        | UpdateError::FetchManifest(_)
        | UpdateError::ManifestStatus(_)
        | UpdateError::DownloadArtifact(_)
        | UpdateError::ArtifactStatus(_)) => network(error),
        error @ (UpdateError::ManifestTooLarge
        | UpdateError::ParseManifest(_)
        | UpdateError::UnsupportedManifestSchema(_)
        | UpdateError::EmptyArtifactCatalog
        | UpdateError::InvalidChecksum
        | UpdateError::MissingArtifact(_)) => manifest(error),
        error @ (UpdateError::ArtifactTooLarge
        | UpdateError::ChecksumMismatch
        | UpdateError::CandidateRejected
        | UpdateError::CandidateVersionMismatch { .. }) => verification(error),
        error @ (UpdateError::CreateCandidate(_)
        | UpdateError::WriteCandidate(_)
        | UpdateError::SetCandidatePermissions(_)
        | UpdateError::ExecuteCandidate(_)) => candidate(error),
        error @ (UpdateError::ReplaceExecutable(_)
        | UpdateError::RemoveCandidate(_)
        | UpdateError::Restart(_)) => replacement(error),
        error @ (UpdateError::CreateConfigDirectory(_)
        | UpdateError::WriteState(_)
        | UpdateError::ReadState(_)
        | UpdateError::ParseState(_)
        | UpdateError::UnsupportedStateSchema(_)
        | UpdateError::SerializeState(_)) => state(error),
        error @ (UpdateError::SystemClock(_) | UpdateError::Prompt(_)) => prompt_and_clock(error),
    }
}

fn configuration(error: &UpdateError) -> Diagnostic {
    match error {
        UpdateError::UpdateChannelUnavailable
        | UpdateError::Version(_)
        | UpdateError::InvalidUrl { .. }
        | UpdateError::InsecureUrl(_) => {
            Diagnostic::general(DiagnosticReason::InvalidConfiguration)
        }
        UpdateError::MissingConfigDirectory => {
            Diagnostic::general(DiagnosticReason::MissingDirectory)
        }
        _ => unreachable!("unexpected update configuration error"),
    }
}

fn network(error: &UpdateError) -> Diagnostic {
    match error {
        UpdateError::BuildClient(_)
        | UpdateError::FetchManifest(_)
        | UpdateError::DownloadArtifact(_) => {
            Diagnostic::general(DiagnosticReason::NetworkRequestFailed)
        }
        UpdateError::ManifestStatus(status) => {
            http(DiagnosticOperation::FetchUpdateManifest, *status)
        }
        UpdateError::ArtifactStatus(status) => http(DiagnosticOperation::DownloadUpdate, *status),
        _ => unreachable!("unexpected update network error"),
    }
}

fn http(operation: DiagnosticOperation, status: u16) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::HttpRequestRejected,
        DiagnosticDetails::Http { operation, status },
    )
}

fn manifest(error: &UpdateError) -> Diagnostic {
    let observed_version = match error {
        UpdateError::UnsupportedManifestSchema(version) => Some(u16::from(*version)),
        _ => None,
    };
    Diagnostic::new(
        DiagnosticReason::InvalidManifest,
        DiagnosticDetails::Schema {
            document: DocumentKind::UpdateManifest,
            observed_version,
        },
    )
}

fn verification(_error: &UpdateError) -> Diagnostic {
    Diagnostic::general(DiagnosticReason::UpdateVerificationFailed)
}

fn candidate(error: &UpdateError) -> Diagnostic {
    let (UpdateError::CreateCandidate(source)
    | UpdateError::WriteCandidate(source)
    | UpdateError::SetCandidatePermissions(source)
    | UpdateError::ExecuteCandidate(source)) = error
    else {
        unreachable!("unexpected update candidate error")
    };
    details::io(DiagnosticOperation::VerifyUpdate, source)
}

fn replacement(error: &UpdateError) -> Diagnostic {
    let (UpdateError::ReplaceExecutable(source)
    | UpdateError::RemoveCandidate(source)
    | UpdateError::Restart(source)) = error
    else {
        unreachable!("unexpected update replacement error")
    };
    Diagnostic::new(
        DiagnosticReason::UpdateReplacementFailed,
        DiagnosticDetails::Io {
            operation: DiagnosticOperation::ReplaceExecutable,
            error_kind: IoErrorKind::from_std(source.kind()),
        },
    )
}

fn state(error: &UpdateError) -> Diagnostic {
    match error {
        UpdateError::CreateConfigDirectory(_) | UpdateError::WriteState(_) => {
            let (UpdateError::CreateConfigDirectory(source) | UpdateError::WriteState(source)) =
                error
            else {
                unreachable!("unexpected update state write error")
            };
            details::io(DiagnosticOperation::WriteConfiguration, source)
        }
        UpdateError::ReadState(source) => {
            details::io(DiagnosticOperation::ReadConfiguration, source)
        }
        UpdateError::ParseState(_) | UpdateError::UnsupportedStateSchema(_) => {
            let observed_version = match error {
                UpdateError::UnsupportedStateSchema(version) => Some(u16::from(*version)),
                _ => None,
            };
            Diagnostic::new(
                DiagnosticReason::InvalidConfiguration,
                DiagnosticDetails::Schema {
                    document: DocumentKind::UpdateState,
                    observed_version,
                },
            )
        }
        UpdateError::SerializeState(_) => {
            Diagnostic::general(DiagnosticReason::SerializationFailed)
        }
        _ => unreachable!("unexpected update state error"),
    }
}

fn prompt_and_clock(error: &UpdateError) -> Diagnostic {
    match error {
        UpdateError::SystemClock(_) => Diagnostic::general(DiagnosticReason::InternalInvariant),
        UpdateError::Prompt(source) => Diagnostic::new(
            DiagnosticReason::UserPromptFailed,
            DiagnosticDetails::Io {
                operation: DiagnosticOperation::ReplaceExecutable,
                error_kind: IoErrorKind::from_std(source.kind()),
            },
        ),
        _ => unreachable!("unexpected update prompt or clock error"),
    }
}
