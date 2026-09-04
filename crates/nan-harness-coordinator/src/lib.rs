#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::expect_used, clippy::unwrap_used))]

mod capture;
mod client;
mod daemon;
mod diagnostics;
mod error;
mod paths;
mod protocol;
mod scheduler;

pub use capture::{CaptureLeg, CaptureRequest, CaptureSink};
pub use client::{CoordinatorClient, RequestLease, RetryDirective};
pub use daemon::run_daemon;
pub use diagnostics::{
    DiagnosticsStatus, disable_diagnostics, enable_diagnostics, purge_diagnostics,
    read_diagnostics_status,
};
pub use error::CoordinatorError;
pub use paths::config_directory;
pub use protocol::{AttemptOutcome, EndpointKind};
