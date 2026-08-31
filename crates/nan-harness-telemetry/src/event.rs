use crate::consent::InstallationId;
use crate::consent::ReportConsent;
use crate::diagnostic::{Diagnostic, DiagnosticReason};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const APPLICATION_NAME: &str = "nan-harness";

pub const REOPEN_TERMINAL_GUIDANCE_TEXT: &str = "The current terminal session cannot access the project directory. Please close this terminal, open a new terminal in the project directory, and try again.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorReport {
    schema_version: u8,
    report_id: String,
    timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    installation_id: Option<InstallationId>,
    application: Application,
    failure: Failure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostic: Option<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_guidance: Option<UserGuidance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<HarnessIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<Transport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<OperationContext>,
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
    pub fn new(
        context: ErrorReportContext,
        consent: ReportConsent,
        installation_id: InstallationId,
    ) -> Result<Self, EventError> {
        Ok(Self {
            schema_version: 3,
            report_id: generate_report_id()?,
            timestamp: timestamp(OffsetDateTime::now_utc())?,
            installation_id: Some(installation_id),
            application: Application {
                name: APPLICATION_NAME.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                build_commit: option_env!("NAN_BUILD_COMMIT")
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
            },
            failure: context.failure,
            diagnostic: Some(context.diagnostic),
            user_guidance: context.user_guidance,
            harness: context.harness,
            transport: context.transport,
            operation: context.operation,
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
    pub fn installation_id(&self) -> Option<&InstallationId> {
        self.installation_id.as_ref()
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
    pub fn diagnostic(&self) -> Option<&Diagnostic> {
        self.diagnostic.as_ref()
    }

    #[must_use]
    pub fn user_guidance(&self) -> Option<&UserGuidance> {
        self.user_guidance.as_ref()
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
    pub fn operation(&self) -> Option<&OperationContext> {
        self.operation.as_ref()
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

    #[must_use]
    pub fn upgrade_for_delivery(mut self, installation_id: InstallationId) -> Self {
        if self.schema_version < 3 {
            self.schema_version = 3;
            self.diagnostic = Some(Diagnostic::legacy());
        }
        self.installation_id = Some(installation_id);
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuidanceClassification {
    Environmental,
}

impl GuidanceClassification {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Environmental => "environmental",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserGuidance {
    classification: GuidanceClassification,
    id: String,
    shown: bool,
    locale: String,
    version: u8,
    text: String,
}

impl UserGuidance {
    #[must_use]
    pub fn reopen_terminal(shown: bool) -> Self {
        Self {
            classification: GuidanceClassification::Environmental,
            id: "reopen-terminal".to_owned(),
            shown,
            locale: "en".to_owned(),
            version: 1,
            text: REOPEN_TERMINAL_GUIDANCE_TEXT.to_owned(),
        }
    }

    #[must_use]
    pub const fn classification(&self) -> GuidanceClassification {
        self.classification
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn shown(&self) -> bool {
        self.shown
    }

    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    #[must_use]
    pub const fn version(&self) -> u8 {
        self.version
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn is_approved(&self) -> bool {
        self.classification == GuidanceClassification::Environmental
            && self.id == "reopen-terminal"
            && self.locale == "en"
            && self.version == 1
            && self.text == REOPEN_TERMINAL_GUIDANCE_TEXT
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReportContext {
    failure: Failure,
    diagnostic: Diagnostic,
    harness: Option<HarnessIdentity>,
    transport: Option<Transport>,
    operation: Option<OperationContext>,
    user_guidance: Option<UserGuidance>,
    interactive: bool,
    stack: Vec<StackFrame>,
}

impl ErrorReportContext {
    #[must_use]
    pub fn new(failure: Failure, interactive: bool) -> Self {
        Self {
            failure,
            diagnostic: Diagnostic::unclassified(),
            harness: None,
            transport: None,
            operation: None,
            user_guidance: None,
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
    pub fn with_diagnostic(mut self, diagnostic: Diagnostic) -> Self {
        self.diagnostic = diagnostic;
        self
    }

    #[must_use]
    pub fn with_transport(mut self, transport: Transport) -> Self {
        self.transport = Some(transport);
        self
    }

    #[must_use]
    pub fn with_operation(mut self, operation: OperationContext) -> Self {
        self.operation = Some(operation);
        self
    }

    #[must_use]
    pub fn with_user_guidance(mut self, user_guidance: UserGuidance) -> Self {
        self.user_guidance = Some(user_guidance);
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

    #[must_use]
    pub const fn diagnostic_reason(&self) -> DiagnosticReason {
        self.diagnostic.reason()
    }

    #[must_use]
    pub fn user_guidance(&self) -> Option<&UserGuidance> {
        self.user_guidance.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Application {
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_commit: Option<String>,
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

    #[must_use]
    pub fn build_commit(&self) -> Option<&str> {
        self.build_commit.as_deref()
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
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<FailureCause>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
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
            cause: None,
            http_status: None,
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
            cause: Some(FailureCause::Internal),
            http_status: None,
        }
    }

    #[must_use]
    pub const fn with_cause(mut self, cause: FailureCause) -> Self {
        self.cause = Some(cause);
        self
    }

    #[must_use]
    pub const fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
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

    #[must_use]
    pub fn cause(&self) -> Option<FailureCause> {
        self.cause
    }

    #[must_use]
    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCause {
    MissingExecutable,
    NotFound,
    UnsupportedVersion,
    MissingCredential,
    InvalidConfiguration,
    PermissionDenied,
    Filesystem,
    Network,
    Timeout,
    HttpStatus,
    InvalidResponse,
    ProcessStart,
    ProcessExit,
    Serialization,
    InvalidData,
    Internal,
}

impl FailureCause {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingExecutable => "missing-executable",
            Self::NotFound => "not-found",
            Self::UnsupportedVersion => "unsupported-version",
            Self::MissingCredential => "missing-credential",
            Self::InvalidConfiguration => "invalid-configuration",
            Self::PermissionDenied => "permission-denied",
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::Timeout => "timeout",
            Self::HttpStatus => "http-status",
            Self::InvalidResponse => "invalid-response",
            Self::ProcessStart => "process-start",
            Self::ProcessExit => "process-exit",
            Self::Serialization => "serialization",
            Self::InvalidData => "invalid-data",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCategory {
    Configuration,
    Discovery,
    Planning,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityStatus>,
}

impl HarnessIdentity {
    #[must_use]
    pub fn new(kind: HarnessKind, version: Option<String>) -> Self {
        Self {
            kind,
            version,
            compatibility: None,
        }
    }

    #[must_use]
    pub const fn with_compatibility(mut self, compatibility: CompatibilityStatus) -> Self {
        self.compatibility = Some(compatibility);
        self
    }

    #[must_use]
    pub fn kind(&self) -> HarnessKind {
        self.kind
    }

    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    #[must_use]
    pub fn compatibility(&self) -> Option<CompatibilityStatus> {
        self.compatibility
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityStatus {
    Tested,
    Supported,
    NewerUntested,
    OlderUnsupported,
    Unparseable,
}

impl CompatibilityStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tested => "tested",
            Self::Supported => "supported",
            Self::NewerUntested => "newer-untested",
            Self::OlderUnsupported => "older-unsupported",
            Self::Unparseable => "unparseable",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessKind {
    ClaudeCode,
    ChatGptDesktop,
    ClaudeDesktop,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Hermes,
    HermesDesktop,
    Pi,
    PrimeAgent,
    #[serde(rename = "deepseek-harness")]
    DeepSeekHarness,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Cline,
    QwenCode,
    KimiCode,
    Aider,
    Goose,
    Fx,
}

impl HarnessKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ChatGptDesktop => "chatgpt-desktop",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::HermesDesktop => "hermes-desktop",
            Self::Pi => "pi",
            Self::PrimeAgent => "prime-agent",
            Self::DeepSeekHarness => "deepseek-harness",
            Self::OpenClaw => "openclaw",
            Self::Cline => "cline",
            Self::QwenCode => "qwen-code",
            Self::KimiCode => "kimi-code",
            Self::Aider => "aider",
            Self::Goose => "goose",
            Self::Fx => "fx",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
    FxGatewayBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationContext {
    kind: OperationKind,
}

impl OperationContext {
    #[must_use]
    pub const fn new(kind: OperationKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub fn kind(&self) -> OperationKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationKind {
    HarnessRun,
    HarnessDryRun,
    HarnessConfig,
    HarnessConfigRemove,
    Doctor,
    Update,
    Uninstall,
    TelemetryConfiguration,
}

impl OperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HarnessRun => "harness-run",
            Self::HarnessDryRun => "harness-dry-run",
            Self::HarnessConfig => "harness-config",
            Self::HarnessConfigRemove => "harness-config-remove",
            Self::Doctor => "doctor",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::TelemetryConfiguration => "telemetry-configuration",
        }
    }
}

impl Transport {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectChat => "direct-chat",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ResponsesBridge => "responses-bridge",
            Self::FxGatewayBridge => "fx-gateway-bridge",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeContext {
    os_family: OsFamily,
    architecture: Architecture,
    #[serde(default)]
    target_environment: TargetEnvironment,
    interactive: bool,
}

impl RuntimeContext {
    pub(crate) fn current(interactive: bool) -> Self {
        Self {
            os_family: OsFamily::current(),
            architecture: Architecture::current(),
            target_environment: TargetEnvironment::current(),
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
    pub fn target_environment(&self) -> TargetEnvironment {
        self.target_environment
    }

    #[must_use]
    pub fn interactive(&self) -> bool {
        self.interactive
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetEnvironment {
    Gnu,
    Musl,
    Msvc,
    #[default]
    Other,
}

impl TargetEnvironment {
    const fn current() -> Self {
        if cfg!(target_env = "gnu") {
            Self::Gnu
        } else if cfg!(target_env = "musl") {
            Self::Musl
        } else if cfg!(target_env = "msvc") {
            Self::Msvc
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gnu => "gnu",
            Self::Musl => "musl",
            Self::Msvc => "msvc",
            Self::Other => "other",
        }
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
