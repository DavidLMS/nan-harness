use crate::{BridgeModelPolicy, BridgeReasoningRequest, error::ApiError};
use nan_harness_core::model::ReasoningEffort;
use nan_harness_core::{ReasoningPolicy, ReasoningSelection};
use serde_json::{Map, Value, json};

pub(crate) fn adjusted(policy: ReasoningPolicy, disabled: bool) -> bool {
    policy == ReasoningPolicy::AlwaysOn && disabled
}

pub(crate) fn mismatch(
    model_id: &str,
    policy: ReasoningPolicy,
    requested: BridgeReasoningRequest,
) -> ApiError {
    let policy = match policy {
        ReasoningPolicy::Unsupported => BridgeModelPolicy::Unsupported,
        ReasoningPolicy::Toggle { .. } => BridgeModelPolicy::Toggle,
        ReasoningPolicy::Effort { .. } => BridgeModelPolicy::Effort,
        ReasoningPolicy::AlwaysOn => BridgeModelPolicy::AlwaysOn,
        ReasoningPolicy::Unknown => BridgeModelPolicy::Unknown,
    };
    ApiError::ReasoningPolicyMismatch {
        model_id: model_id.to_owned(), requested, policy,
        message: "requested reasoning is incompatible with this model; select an available reasoning level".to_owned(),
    }
}

pub(crate) fn diagnostic_effort(value: &str) -> BridgeReasoningRequest {
    match value {
        "none" => BridgeReasoningRequest::None,
        "low" => BridgeReasoningRequest::Low,
        "medium" => BridgeReasoningRequest::Medium,
        "high" => BridgeReasoningRequest::High,
        "xhigh" | "max" => BridgeReasoningRequest::Xhigh,
        _ => BridgeReasoningRequest::Other,
    }
}

pub(crate) fn diagnostic_selection(selection: ReasoningSelection) -> BridgeReasoningRequest {
    match selection {
        ReasoningSelection::Auto => BridgeReasoningRequest::Auto,
        ReasoningSelection::Toggle(false) => BridgeReasoningRequest::None,
        ReasoningSelection::Toggle(true) | ReasoningSelection::Effort(ReasoningEffort::High) => {
            BridgeReasoningRequest::High
        }
        ReasoningSelection::Effort(ReasoningEffort::Low) => BridgeReasoningRequest::Low,
        ReasoningSelection::Effort(ReasoningEffort::Medium) => BridgeReasoningRequest::Medium,
        ReasoningSelection::Effort(ReasoningEffort::Max) => BridgeReasoningRequest::Xhigh,
    }
}

/// Serialize the validated selection using the model's control contract.
/// Always-on reasoning needs no wire control; a toggle is a different API
/// capability from disabling an effort-based model.
pub(crate) fn apply(
    body: &mut Map<String, Value>,
    policy: ReasoningPolicy,
    selection: ReasoningSelection,
) {
    match (policy, selection) {
        (ReasoningPolicy::Effort { .. }, ReasoningSelection::Toggle(false)) => {
            body.insert("reasoning_effort".to_owned(), json!("none"));
        }
        (ReasoningPolicy::Toggle { .. }, ReasoningSelection::Toggle(enabled)) => {
            body.insert(
                "chat_template_kwargs".to_owned(),
                json!({"enable_thinking": enabled}),
            );
        }
        (ReasoningPolicy::Effort { .. }, ReasoningSelection::Effort(effort)) => {
            body.insert("reasoning_effort".to_owned(), json!(effort));
        }
        _ => {}
    }
}
