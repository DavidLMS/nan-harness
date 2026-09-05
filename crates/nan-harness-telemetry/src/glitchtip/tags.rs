use crate::diagnostic::{
    AttemptBucket, DiagnosticDetails, ModelPolicy, ReasoningRequest, RecoveryOutcome,
    RequestPriority, TimeoutPhase,
};
use std::collections::BTreeMap;

pub(super) fn add_diagnostic_tags(
    tags: &mut BTreeMap<&'static str, String>,
    details: &DiagnosticDetails,
) {
    match details {
        DiagnosticDetails::Bridge {
            endpoint,
            model_id,
            requested_reasoning,
            model_policy,
            timeout_phase,
            recovery_outcome,
            attempt,
            priority,
            cache_replay_detected,
            cache_bypass_attempted,
        } => {
            tags.insert("diagnostic.endpoint", endpoint.as_str().to_owned());
            insert_optional(tags, "diagnostic.model_id", model_id.as_deref());
            insert_optional(
                tags,
                "diagnostic.requested_reasoning",
                requested_reasoning.map(ReasoningRequest::as_str),
            );
            insert_optional(
                tags,
                "diagnostic.model_policy",
                model_policy.map(ModelPolicy::as_str),
            );
            insert_optional(
                tags,
                "diagnostic.timeout_phase",
                timeout_phase.map(TimeoutPhase::as_str),
            );
            insert_optional(
                tags,
                "diagnostic.recovery",
                recovery_outcome.map(RecoveryOutcome::as_str),
            );
            insert_optional(
                tags,
                "diagnostic.attempt",
                attempt.map(AttemptBucket::as_str),
            );
            insert_optional(
                tags,
                "diagnostic.priority",
                priority.map(RequestPriority::as_str),
            );
            insert_optional(tags, "diagnostic.cache_replay", *cache_replay_detected);
            insert_optional(tags, "diagnostic.cache_bypass", *cache_bypass_attempted);
        }
        DiagnosticDetails::Io {
            operation,
            error_kind,
        } => {
            add_operation_tag(tags, operation.as_str());
            tags.insert("diagnostic.io_kind", error_kind.as_str().to_owned());
        }
        DiagnosticDetails::Process { operation, .. }
        | DiagnosticDetails::Http { operation, .. } => add_operation_tag(tags, operation.as_str()),
        DiagnosticDetails::General
        | DiagnosticDetails::Version { .. }
        | DiagnosticDetails::Schema { .. } => {}
    }
}

fn add_operation_tag(tags: &mut BTreeMap<&'static str, String>, operation: &'static str) {
    tags.insert("diagnostic.operation", operation.to_owned());
}

fn insert_optional<T: ToString>(
    tags: &mut BTreeMap<&'static str, String>,
    name: &'static str,
    value: Option<T>,
) {
    if let Some(value) = value {
        tags.insert(name, value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::add_diagnostic_tags;
    use crate::diagnostic::{
        AttemptBucket, BridgeEndpoint, DiagnosticDetails, ModelPolicy, ReasoningRequest,
        RecoveryOutcome, RequestPriority, TimeoutPhase,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn bridge_tags_serialize_the_complete_allowlist_exactly() {
        let details = DiagnosticDetails::Bridge {
            endpoint: BridgeEndpoint::Responses,
            model_id: Some("model-alpha".to_owned()),
            requested_reasoning: Some(ReasoningRequest::Xhigh),
            model_policy: Some(ModelPolicy::AlwaysOn),
            timeout_phase: Some(TimeoutPhase::InitialResponse),
            recovery_outcome: Some(RecoveryOutcome::Retrying),
            attempt: Some(AttemptBucket::First),
            priority: Some(RequestPriority::Foreground),
            cache_replay_detected: Some(true),
            cache_bypass_attempted: Some(false),
        };
        let mut tags = BTreeMap::new();
        add_diagnostic_tags(&mut tags, &details);

        assert_eq!(
            serde_json::to_value(tags).expect("tags should serialize"),
            json!({
                "diagnostic.endpoint": "responses",
                "diagnostic.model_id": "model-alpha",
                "diagnostic.requested_reasoning": "xhigh",
                "diagnostic.model_policy": "always-on",
                "diagnostic.timeout_phase": "initial-response",
                "diagnostic.recovery": "retrying",
                "diagnostic.attempt": "first",
                "diagnostic.priority": "foreground",
                "diagnostic.cache_replay": "true",
                "diagnostic.cache_bypass": "false"
            })
        );
    }

    #[test]
    fn optional_bridge_tags_are_absent_not_empty_or_null() {
        let details = DiagnosticDetails::Bridge {
            endpoint: BridgeEndpoint::Models,
            model_id: None,
            requested_reasoning: None,
            model_policy: None,
            timeout_phase: None,
            recovery_outcome: None,
            attempt: None,
            priority: None,
            cache_replay_detected: None,
            cache_bypass_attempted: None,
        };
        let mut tags = BTreeMap::new();
        add_diagnostic_tags(&mut tags, &details);

        assert_eq!(tags.len(), 1);
        assert_eq!(tags.get("diagnostic.endpoint"), Some(&"models".to_owned()));
        assert!(
            !serde_json::to_string(&tags)
                .expect("tags should serialize")
                .contains("null")
        );
    }
}
