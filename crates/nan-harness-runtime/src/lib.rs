#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::expect_used, clippy::unwrap_used))]

mod chat_gateway;
pub mod claude_desktop;
pub mod codex_desktop;
pub mod compatibility;
pub mod config;
pub mod desktop_compatibility;
pub mod discovery;
mod prepared;
mod process;
mod search_policy;
pub mod signals;
pub mod supervisor;
pub mod temporary;
pub mod update;

pub use nan_harness_bridge::{
    BridgeActivity, BridgeAttemptBucket, BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint,
    BridgeError, BridgeModelPolicy, BridgeReasoningRequest, BridgeRecoveryOutcome,
    BridgeRequestPriority, BridgeTimeoutPhase, ClaudeAutoModeReviewStage,
    ClaudeAutoModeTracePayload, ModelUsageSnapshot, ProviderUsageSnapshot,
};

pub use chat_gateway::{
    ChatGatewayError, RunningChatCompletionsGateway, start_chat_completions_gateway,
};
pub use claude_desktop::{
    ClaudeDesktopBridgeError, RunningClaudeDesktopBridge, start_claude_desktop_bridge,
};
pub use codex_desktop::{
    CodexDesktopBridgeError, RunningCodexDesktopBridge, start_codex_desktop_bridge,
};

pub use compatibility::{
    CompatibilityError, RefreshOutcome, automatic_refresh_enabled, compatibility_manifest_url,
    refresh_compatibility_manifest,
};
pub use config::{
    ConfigError, ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment,
    ResolvedConfig,
};
pub use desktop_compatibility::{
    DesktopCompatibilityEntry, DesktopCompatibilityError, DesktopCompatibilityEvidence,
    DesktopCompatibilityReport, DesktopCompatibilityStatus, classify_desktop_version,
    desktop_compatibility, desktop_platform, evaluate_desktop_compatibility,
};
pub use discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, bundled_compatibility_manifest,
    discover_harness, inspect_harness, is_executable_file, locate_harness_executable,
};
pub use prepared::PreparedError;
pub use process::ProcessError;
pub use search_policy::{SearchConfiguration, SearchPolicyError, inspect_search_configuration};
pub use signals::{CancellationToken, SignalKind};
pub use supervisor::{ExecutionOutcome, ExecutionReport, LaunchSession, RuntimeError, Supervisor};
