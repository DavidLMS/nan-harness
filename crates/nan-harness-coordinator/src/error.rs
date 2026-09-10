use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not prepare private coordinator state at '{}': {source}", path.display())]
    State {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not encode coordinator state: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("could not generate private coordinator state: {0}")]
    Random(#[from] getrandom::Error),
    #[error("coordinator control protocol failed: {0}")]
    Protocol(&'static str),
    #[error("diagnostic captures are still being written; retry after active requests finish")]
    CaptureBusy,
    #[error(
        "coordinator protocol v{detected} is still active; close sessions started by the previous nan-harness version and retry after its coordinator exits"
    )]
    IncompatibleDaemon { detected: u8 },
    #[error("timed out waiting for coordinated provider capacity")]
    QueueTimeout,
    #[error("launch token budget exhausted ({consumed} of {limit} tokens)")]
    BudgetExhausted { consumed: u64, limit: u64 },
    #[error("token accounting is unavailable for launch '{launch_id}'")]
    AccountingUnavailable { launch_id: String },
    #[error("inconsistent token budget for launch '{launch_id}'")]
    BudgetMismatch { launch_id: String },
}

impl CoordinatorError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingConfigDirectory => "NH-COORD-001",
            Self::State { .. } => "NH-COORD-002",
            Self::Encode(_) => "NH-COORD-003",
            Self::Random(_) => "NH-COORD-004",
            Self::Protocol(_) => "NH-COORD-005",
            Self::CaptureBusy => "NH-COORD-006",
            Self::IncompatibleDaemon { .. } => "NH-COORD-007",
            Self::QueueTimeout => "NH-COORD-008",
            Self::BudgetExhausted { .. } => "NH-COORD-009",
            Self::AccountingUnavailable { .. } => "NH-COORD-010",
            Self::BudgetMismatch { .. } => "NH-COORD-011",
        }
    }
}
