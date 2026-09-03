use crate::report::ReportError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AggregateError {
    #[error("could not read report directory '{}': {source}", path.display())]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("report directory '{}' contains no JSON reports", .0.display())]
    NoReports(PathBuf),
    #[error(transparent)]
    Report(#[from] ReportError),
    #[error("could not read aggregate state '{}': {source}", path.display())]
    ReadState {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse aggregate state '{}': {source}", path.display())]
    ParseState {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("aggregate state schema {0} is unsupported")]
    UnsupportedStateSchema(u8),
    #[error("aggregate output path '{}' has no parent directory", .0.display())]
    InvalidOutputPath(PathBuf),
    #[error("could not create aggregate directory '{}': {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize aggregate output: {0}")]
    Serialize(serde_json::Error),
    #[error("could not write aggregate output '{}': {source}", path.display())]
    WriteOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not format aggregate timestamp: {0}")]
    Timestamp(time::error::Format),
}
