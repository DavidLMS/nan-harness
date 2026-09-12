//! Official distribution metadata and conservative, read-only installation discovery.

pub(crate) mod architecture;
mod discovery;
pub mod frozen;
mod versions;

pub use discovery::{discover, inspect};
pub(crate) use discovery::app_name as app_bundle_name;

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
