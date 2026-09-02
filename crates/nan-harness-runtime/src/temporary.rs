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
    InvalidArtifact { artifact_id: String, reason: String },
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
