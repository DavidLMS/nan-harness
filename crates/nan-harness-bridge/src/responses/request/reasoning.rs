use super::wire::ResponsesReasoning;
use crate::error::ApiError;
use crate::{BridgeModelPolicy, BridgeReasoningRequest};
use nan_harness_core::model::{
    CodingModelProfile, ReasoningHint, ReasoningPolicy, ReasoningSelection,
};

pub(super) fn validate_reasoning(
    request: Option<&ResponsesReasoning>,
    model: &CodingModelProfile,
) -> Result<ReasoningSelection, ApiError> {
    let Some(effort) = request.and_then(|request| request.effort.as_deref()) else {
        return Ok(ReasoningSelection::Auto);
    };
    let policy = model.reasoning;
    let hint = match effort {
        "none" => ReasoningHint::Disabled,
        "low" => ReasoningHint::Low,
        "medium" => ReasoningHint::Medium,
        "high" => ReasoningHint::High,
        "xhigh" | "max" => ReasoningHint::ExtraHigh,
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported reasoning effort '{other}'"
            )));
        }
    };
    policy
        .resolve_hint(hint)
        .ok_or_else(|| ApiError::ReasoningPolicyMismatch {
            model_id: model.id.clone(),
            requested: diagnostic_reasoning_request(effort),
            policy: diagnostic_model_policy(policy),
            message: format!("reasoning effort '{effort}' is incompatible with model policy"),
        })
}

fn diagnostic_reasoning_request(value: &str) -> BridgeReasoningRequest {
    match value {
        "none" => BridgeReasoningRequest::None,
        "low" => BridgeReasoningRequest::Low,
        "medium" => BridgeReasoningRequest::Medium,
        "high" => BridgeReasoningRequest::High,
        "xhigh" | "max" => BridgeReasoningRequest::Xhigh,
        _ => BridgeReasoningRequest::Other,
    }
}

const fn diagnostic_model_policy(policy: ReasoningPolicy) -> BridgeModelPolicy {
    match policy {
        ReasoningPolicy::Unsupported => BridgeModelPolicy::Unsupported,
        ReasoningPolicy::Toggle { .. } => BridgeModelPolicy::Toggle,
        ReasoningPolicy::Effort { .. } => BridgeModelPolicy::Effort,
        ReasoningPolicy::AlwaysOn => BridgeModelPolicy::AlwaysOn,
        ReasoningPolicy::Unknown => BridgeModelPolicy::Unknown,
    }
}
