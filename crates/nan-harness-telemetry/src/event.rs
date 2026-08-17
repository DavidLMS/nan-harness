use crate::consent::ReportConsent;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const APPLICATION_NAME: &str = "nan-harness";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorReport {
    schema_version: u8,
    report_id: String,
    timestamp: String,
    application: Application,
    failure: Failure,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<HarnessIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<Transport>,
    runtime: RuntimeContext,
    consent: ReportConsent,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stack: Vec<StackFrame>,
}

impl ErrorReport {
    /// Creates a report containing only fields allowed by the telemetry contract.
    ///
    /// # Errors
    ///
    /// Returns [`EventError`] when a report identifier or timestamp cannot be generated.
    pub fn new(context: ErrorReportContext, consent: ReportConsent) -> Result<Self, EventError> {
        Ok(Self {
            schema_version: 1,
            report_id: generate_report_id()?,
            timestamp: timestamp(OffsetDateTime::now_utc())?,
            application: Application {
                name: APPLICATION_NAME.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            failure: context.failure,
            harness: context.harness,
            transport: context.transport,
            runtime: RuntimeContext::current(context.interactive),
            consent,
            stack: context.stack,
        })
    }

    #[must_use]
    pub fn schema_version(&self) -> u8 {
        self.schema_version
    }

    #[must_use]
    pub fn report_id(&self) -> &str {
        &self.report_id
    }

    #[must_use]
    pub fn timestamp(&self) -> &str {
        &self.timestamp
    }

    #[must_use]
    pub fn application(&self) -> &Application {
        &self.application
    }

    #[must_use]
    pub fn failure(&self) -> &Failure {
        &self.failure
    }

    #[must_use]
    pub fn harness(&self) -> Option<&HarnessIdentity> {
        self.harness.as_ref()
    }

    #[must_use]
    pub fn transport(&self) -> Option<Transport> {
        self.transport
    }

    #[must_use]
    pub fn runtime(&self) -> &RuntimeContext {
        &self.runtime
    }

    #[must_use]
    pub fn consent(&self) -> ReportConsent {
        self.consent
    }

    #[must_use]
    pub fn stack(&self) -> &[StackFrame] {
        &self.stack
    }

    #[must_use]
    pub fn with_consent(mut self, consent: ReportConsent) -> Self {
        self.consent = consent;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReportContext {
    failure: Failure,
    harness: Option<HarnessIdentity>,
    transport: Option<Transport>,
    interactive: bool,
    stack: Vec<StackFrame>,
}

impl ErrorReportContext {
    #[must_use]
    pub fn new(failure: Failure, interactive: bool) -> Self {
        Self {
            failure,
            harness: None,
            transport: None,
            interactive,
            stack: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_harness(mut self, harness: HarnessIdentity) -> Self {
        self.harness = Some(harness);
        self
    }

    #[must_use]
    pub fn with_transport(mut self, transport: Transport) -> Self {
        self.transport = Some(transport);
        self
    }

    #[must_use]
    pub fn with_stack(mut self, stack: Vec<StackFrame>) -> Self {
        self.stack = stack;
        self
    }

    #[must_use]
    pub fn interactive(&self) -> bool {
        self.interactive
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Application {
    name: String,
    version: String,
}

impl Application {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Failure {
    code: String,
    category: FailureCategory,
    stage: FailureStage,
    panic: bool,
    retryable: bool,
}

impl Failure {
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        category: FailureCategory,
        stage: FailureStage,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            stage,
            panic: false,
            retryable,
        }
    }

    #[must_use]
    pub fn panic() -> Self {
        Self {
            code: "NH-INTERNAL-001".to_owned(),
            category: FailureCategory::Internal,
            stage: FailureStage::HarnessExecution,
            panic: true,
            retryable: false,
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn category(&self) -> FailureCategory {
        self.category
    }

    #[must_use]
    pub fn stage(&self) -> FailureStage {
        self.stage
    }

    #[must_use]
    pub fn is_panic(&self) -> bool {
        self.panic
    }

    #[must_use]
    pub fn retryable(&self) -> bool {
        self.retryable
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCategory {
    Configuration,
    Discovery,
    Planning,
    Validation,
    Bridge,
    Provider,
    Process,
    Tool,
    Cleanup,
    Internal,
}

impl FailureCategory {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Discovery => "discovery",
            Self::Planning => "planning",
            Self::Validation => "validation",
            Self::Bridge => "bridge",
            Self::Provider => "provider",
            Self::Process => "process",
            Self::Tool => "tool",
            Self::Cleanup => "cleanup",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureStage {
    Startup,
    CredentialResolution,
    ModelDiscovery,
    HarnessDetection,
    LaunchPlanning,
    LaunchValidation,
    BridgeStartup,
    RequestTranslation,
    HarnessExecution,
    ToolExecution,
    Shutdown,
}

impl FailureStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::CredentialResolution => "credential-resolution",
            Self::ModelDiscovery => "model-discovery",
            Self::HarnessDetection => "harness-detection",
            Self::LaunchPlanning => "launch-planning",
            Self::LaunchValidation => "launch-validation",
            Self::BridgeStartup => "bridge-startup",
            Self::RequestTranslation => "request-translation",
            Self::HarnessExecution => "harness-execution",
            Self::ToolExecution => "tool-execution",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessIdentity {
    kind: HarnessKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

impl HarnessIdentity {
    #[must_use]
    pub fn new(kind: HarnessKind, version: Option<String>) -> Self {
        Self { kind, version }
    }

    #[must_use]
    pub fn kind(&self) -> HarnessKind {
        self.kind
    }

    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessKind {
    ClaudeCode,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Hermes,
    Pi,
    PrimeAgent,
    #[serde(rename = "deepseek-harness")]
    DeepSeekHarness,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Cline,
    QwenCode,
    Aider,
    Goose,
}

impl HarnessKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "deepseek-harness",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen-code",
            Self::Aider => "aider",
            Self::Goose => "goose",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
}

impl Transport {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectChat => "direct-chat",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ResponsesBridge => "responses-bridge",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeContext {
    os_family: OsFamily,
    architecture: Architecture,
    interactive: bool,
}

impl RuntimeContext {
    fn current(interactive: bool) -> Self {
        Self {
            os_family: OsFamily::current(),
            architecture: Architecture::current(),
            interactive,
        }
    }

    #[must_use]
    pub fn os_family(&self) -> OsFamily {
        self.os_family
    }

    #[must_use]
    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    #[must_use]
    pub fn interactive(&self) -> bool {
        self.interactive
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OsFamily {
    Linux,
    Macos,
    Windows,
    Other,
}

impl OsFamily {
    const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Architecture {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "other")]
    Other,
}

impl Architecture {
    const fn current() -> Self {
        if cfg!(target_arch = "x86_64") {
            Self::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StackFrame {
    module: String,
    function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_application: Option<bool>,
}

impl StackFrame {
    #[must_use]
    pub fn new(
        module: impl Into<String>,
        function: impl Into<String>,
        in_application: Option<bool>,
    ) -> Self {
        Self {
            module: module.into(),
            function: function.into(),
            in_application,
        }
    }

    #[must_use]
    pub fn module(&self) -> &str {
        &self.module
    }

    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }

    #[must_use]
    pub fn in_application(&self) -> Option<bool> {
        self.in_application
    }
}

fn generate_report_id() -> Result<String, EventError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(EventError::Random)?;
    let mut identifier = String::with_capacity(39);
    identifier.push_str("report_");
    for byte in bytes {
        write!(&mut identifier, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(identifier)
}

fn timestamp(value: OffsetDateTime) -> Result<String, EventError> {
    value.format(&Rfc3339).map_err(EventError::Timestamp)
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("could not generate an error report identifier: {0}")]
    Random(getrandom::Error),
    #[error("could not format the error report timestamp: {0}")]
    Timestamp(time::error::Format),
}
