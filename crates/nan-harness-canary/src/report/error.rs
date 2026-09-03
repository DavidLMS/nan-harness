use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ReportError {
    #[error("could not read canary report '{}': {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse canary report '{}': {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not serialize canary report: {0}")]
    Serialize(serde_json::Error),
    #[error("could not create canary report directory '{}': {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write canary report '{}': {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("canary report path '{}' has no parent directory", .0.display())]
    InvalidPath(PathBuf),
    #[error("canary report schema {0} is unsupported")]
    UnsupportedSchema(u8),
    #[error("legacy canary reports cannot contain observations")]
    LegacyObservations,
    #[error("canary report contains too many observations: {0}")]
    TooManyObservations(usize),
    #[error("canary report field {0} must not be empty")]
    EmptyField(&'static str),
    #[error("canary report must contain at least one check")]
    MissingChecks,
    #[error("canary report field {0} must contain a semantic version")]
    InvalidSemanticVersion(&'static str),
    #[error("canary report field {0} must contain a lowercase or uppercase SHA-256 digest")]
    InvalidSha256(&'static str),
    #[error("canary report field {0} must contain an RFC 3339 timestamp")]
    InvalidTimestamp(&'static str),
    #[error("canary report completion time precedes its start time")]
    InvalidTimeOrder,
    #[error("canary report checks require a name and at least one attempt")]
    InvalidCheck,
    #[error("successful canary report must not contain a failure")]
    UnexpectedFailure,
    #[error("failed canary report must contain a failure")]
    MissingFailure,
    #[error("canary report outcome does not match its check statuses")]
    InconsistentChecks,
}
