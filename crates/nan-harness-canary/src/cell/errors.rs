use crate::credentials;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CellError {
    #[error("could not read cell spec '{}': {source}", path.display())]
    ReadSpec {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse cell spec '{}': {source}", path.display())]
    ParseSpec {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("cell spec schema {0} is unsupported")]
    UnsupportedSpecSchema(u8),
    #[error("cell spec field {0} must not be empty")]
    EmptySpecField(&'static str),
    #[error("cell spec nan-harness version is invalid: {0}")]
    InvalidNanHarnessVersion(String),
    #[error("cell spec must contain at least one step")]
    MissingSteps,
    #[error("cell spec timeouts and attempts must be greater than zero")]
    InvalidTimeout,
    #[error("cell spec contains an empty step name or script")]
    InvalidStep,
    #[error("cell spec field {0} must contain a safe relative path")]
    UnsafeRelativePath(&'static str),
    #[error("cell spec artifact name '{0}' is invalid")]
    InvalidArtifactName(String),
    #[error("cell spec path '{}' has no parent directory", .0.display())]
    InvalidSpecPath(PathBuf),
    #[error("could not create the cell workspace: {0}")]
    CreateWorkspace(std::io::Error),
    #[error("could not create the private log directory: {0}")]
    CreatePrivateLogDirectory(std::io::Error),
    #[error("could not read private cell logs: {0}")]
    ReadPrivateLogs(std::io::Error),
    #[error("could not preserve a private cell log: {0}")]
    PreservePrivateLog(std::io::Error),
    #[error("could not read cell artifact '{}': {source}", path.display())]
    ReadArtifact {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not copy cell artifact '{}': {source}", path.display())]
    CopyArtifact {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read NAN_API_KEY from the canary credential store: {0}")]
    ReadCredential(credentials::CredentialError),
    #[error("could not format a cell timestamp: {0}")]
    Timestamp(time::error::Format),
    #[error(transparent)]
    Report(#[from] crate::report::ReportError),
    #[error("could not serialize the cell report: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("the reproduction spec does not match the original cell report")]
    ReproductionMismatch,
    #[error("the canary failed; safe evidence was written to '{}'", .0.display())]
    CanaryFailed(PathBuf),
}
