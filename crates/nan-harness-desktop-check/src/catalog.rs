//! Official distribution metadata and conservative, read-only installation discovery.

pub(crate) mod architecture;
mod discovery;
mod distributions;
mod versions;

pub use discovery::{discover, inspect};
pub use distributions::{Distribution, PackageFormat, download};

use semver::Version;
use std::path::PathBuf;
use thiserror::Error;

/// Private local inventory; deliberately not serializable as a public report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    pub executable: PathBuf,
    pub app_version: Option<Version>,
    pub runtime_version: Option<Version>,
}

/// Closed, path-free discovery failures suitable for mapping to public reasons.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("multiple Desktop installations were detected")]
    Ambiguous,
    #[error("Desktop installation inventory could not be read")]
    Unreadable,
    #[error("a Desktop installation exists but is incomplete")]
    Incomplete,
    #[error("the platform or installation environment is unsupported")]
    Unsupported,
}
