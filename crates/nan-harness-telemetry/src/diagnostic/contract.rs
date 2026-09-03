use super::{DiagnosticDetails, DiagnosticReason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Diagnostic {
    reason: DiagnosticReason,
    details: DiagnosticDetails,
}

impl Diagnostic {
    #[must_use]
    pub const fn new(reason: DiagnosticReason, details: DiagnosticDetails) -> Self {
        Self { reason, details }
    }

    #[must_use]
    pub const fn general(reason: DiagnosticReason) -> Self {
        Self::new(reason, DiagnosticDetails::General)
    }

    #[must_use]
    pub const fn unclassified() -> Self {
        Self::general(DiagnosticReason::Unclassified)
    }

    #[must_use]
    pub const fn legacy() -> Self {
        Self::general(DiagnosticReason::LegacyReport)
    }

    #[must_use]
    pub const fn reason(&self) -> DiagnosticReason {
        self.reason
    }

    #[must_use]
    pub const fn details(&self) -> &DiagnosticDetails {
        &self.details
    }
}

impl Default for Diagnostic {
    fn default() -> Self {
        Self::unclassified()
    }
}

#[cfg(test)]
mod tests {
    use super::Diagnostic;
    use crate::diagnostic::{DiagnosticDetails, DiagnosticOperation, DiagnosticReason};
    use serde_json::json;

    #[test]
    fn diagnostic_serialization_preserves_the_contract() {
        let diagnostic = Diagnostic::new(
            DiagnosticReason::HttpRequestRejected,
            DiagnosticDetails::Http {
                operation: DiagnosticOperation::DiscoverModels,
                status: 502,
            },
        );
        let expected = json!({
            "reason": "http-request-rejected",
            "details": {
                "kind": "http",
                "operation": "discover-models",
                "status": 502
            }
        });

        assert_eq!(
            serde_json::to_value(&diagnostic).expect("diagnostic should serialize"),
            expected
        );
        assert_eq!(
            serde_json::from_value::<Diagnostic>(expected).expect("diagnostic should deserialize"),
            diagnostic
        );
    }

    #[test]
    fn default_diagnostic_remains_unclassified_and_general() {
        let diagnostic = Diagnostic::default();

        assert_eq!(diagnostic.reason(), DiagnosticReason::Unclassified);
        assert_eq!(diagnostic.details(), &DiagnosticDetails::General);
        assert_eq!(
            serde_json::to_value(diagnostic).expect("diagnostic should serialize"),
            json!({
                "reason": "unclassified",
                "details": { "kind": "general" }
            })
        );
    }

    #[test]
    fn diagnostic_rejects_unknown_contract_fields() {
        let value = json!({
            "reason": "unclassified",
            "details": { "kind": "general" },
            "message": "not part of the telemetry contract"
        });

        assert!(serde_json::from_value::<Diagnostic>(value).is_err());
    }
}
