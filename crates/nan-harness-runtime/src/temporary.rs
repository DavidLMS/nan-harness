use nan_harness_i18n::DiagnosticText;
mod formats;
mod lifecycle;
mod overlays;
mod paths;
mod platform;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use thiserror::Error;

pub use lifecycle::TemporaryWorkspace;

#[derive(Debug, Error)]
pub enum TemporaryError {
    #[error("could not create a private temporary workspace: {0}")]
    CreateWorkspace(std::io::Error),
    #[error("could not resolve the current user's home directory")]
    MissingUserHome,
    #[error("temporary artifact '{artifact_id}' is invalid: {reason}")]
    InvalidArtifact {
        artifact_id: String,
        reason: DiagnosticText,
    },
    #[error("could not materialize temporary artifact '{artifact_id}': {source}")]
    Materialize {
        artifact_id: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not mirror configuration overlay '{overlay_id}': {source}")]
    MirrorOverlay {
        overlay_id: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not set private permissions on '{}': {source}", path.display())]
    Permissions {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for TemporaryError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::CreateWorkspace(field_0) => {
                m::error_temporary_create_workspace(locale, &(field_0))
            }
            Self::MissingUserHome => m::error_temporary_missing_user_home(locale),
            Self::InvalidArtifact {
                artifact_id,
                reason,
            } => m::error_temporary_invalid_artifact(
                locale,
                &(artifact_id),
                &(nan_harness_i18n::TerminalMessage::terminal_message(reason, locale)),
            ),
            Self::Materialize {
                artifact_id,
                source,
            } => m::error_temporary_materialize(locale, &(artifact_id), &(source)),
            Self::MirrorOverlay { overlay_id, source } => {
                m::error_temporary_mirror_overlay(locale, &(overlay_id), &(source))
            }
            Self::Permissions { path, source } => {
                m::error_temporary_permissions(locale, &(source), &(path.display()))
            }
        }
    }
}
