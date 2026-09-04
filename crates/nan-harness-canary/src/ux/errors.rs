use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum UxError {
    #[error("could not parse the embedded UX scenarios: {0}")]
    Parse(serde_json::Error),
    #[error("UX scenario identifier '{0}' is duplicated")]
    DuplicateScenario(String),
    #[error("UX scenario '{0}' is invalid")]
    InvalidScenario(String),
    #[error("unknown UX scenario '{0}'")]
    UnknownScenario(String),
    #[error("UX output path '{}' has no parent directory", .0.display())]
    InvalidOutputPath(PathBuf),
    #[error("could not create UX output directory '{}': {source}", path.display())]
    CreateOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write UX output '{}': {source}", path.display())]
    WriteOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
