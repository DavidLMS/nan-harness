use super::identifiers::{generate_report_id, timestamp};
use super::{
    Application, EventError, Failure, HarnessIdentity, OperationContext, RuntimeContext, Transport,
    UserGuidance,
};
use crate::consent::{InstallationId, ReportConsent};
use crate::diagnostic::{Diagnostic, DiagnosticReason};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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
            schema_version: 4,
            report_id: generate_report_id()?,
            timestamp: timestamp(OffsetDateTime::now_utc())?,
            installation_id: Some(installation_id),
            application: Application::current(),
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
