use serde::{Deserialize, Serialize};

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
