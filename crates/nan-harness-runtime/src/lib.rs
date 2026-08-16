#![forbid(unsafe_code)]

pub mod config;
pub mod discovery;
pub mod process;
pub mod signals;
pub mod supervisor;
pub mod temporary;

pub use config::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, bundled_compatibility_manifest,
    discover_harness,
};
pub use signals::{CancellationToken, SignalKind};
pub use supervisor::{ExecutionOutcome, ExecutionReport, RuntimeError, Supervisor};
