#![forbid(unsafe_code)]

pub mod compatibility;
pub mod config;
pub mod discovery;
mod prepared;
mod process;
pub mod signals;
pub mod supervisor;
pub mod temporary;
pub mod update;

pub use compatibility::{
    CompatibilityError, RefreshOutcome, automatic_refresh_enabled, compatibility_manifest_url,
    refresh_compatibility_manifest,
};
pub use config::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, bundled_compatibility_manifest,
    discover_harness,
};
pub use prepared::PreparedError;
pub use process::ProcessError;
pub use signals::{CancellationToken, SignalKind};
pub use supervisor::{ExecutionOutcome, ExecutionReport, RuntimeError, Supervisor};
