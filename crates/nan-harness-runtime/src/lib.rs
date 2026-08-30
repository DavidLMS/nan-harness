#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::expect_used, clippy::unwrap_used))]

pub mod compatibility;
pub mod config;
pub mod discovery;
mod prepared;
mod process;
mod search_policy;
pub mod signals;
pub mod supervisor;
pub mod temporary;
pub mod update;

pub use nan_harness_bridge::{
    BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint, BridgeError, BridgeModelPolicy,
    BridgeReasoningRequest, ModelUsageSnapshot, ProviderUsageSnapshot,
};

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
    discover_harness, is_executable_file,
};
pub use prepared::PreparedError;
pub use process::ProcessError;
pub use search_policy::SearchPolicyError;
pub use signals::{CancellationToken, SignalKind};
pub use supervisor::{ExecutionOutcome, ExecutionReport, LaunchSession, RuntimeError, Supervisor};
