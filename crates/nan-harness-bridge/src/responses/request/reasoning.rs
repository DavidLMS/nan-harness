use super::wire::ResponsesReasoning;
use crate::error::ApiError;
use crate::{BridgeModelPolicy, BridgeReasoningRequest};
use nan_harness_core::model::{
    CodingModelProfile, ReasoningHint, ReasoningPolicy, ReasoningSelection,
};
use serde_json::{Map, Value, json};

pub(super) fn validate_reasoning(
    request: Option<&ResponsesReasoning>,
    model: &CodingModelProfile,
) -> Result<ReasoningSelection, ApiError> {
    let Some(request) = request else {
        return Ok(ReasoningSelection::Auto);
    };
    let policy = model.reasoning;
    let hint = match request.effort.as_str() {
        "none" => ReasoningHint::Disabled,
        "low" => ReasoningHint::Low,
        "medium" => ReasoningHint::Medium,
        "high" => ReasoningHint::High,
        "xhigh" => ReasoningHint::ExtraHigh,
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
            requested: diagnostic_reasoning_request(&request.effort),
            policy: diagnostic_model_policy(policy),
            message: format!(
                "reasoning effort '{}' is incompatible with model policy",
                request.effort
            ),
        })
}

fn diagnostic_reasoning_request(value: &str) -> BridgeReasoningRequest {
    match value {
        "none" => BridgeReasoningRequest::None,
        "low" => BridgeReasoningRequest::Low,
        "medium" => BridgeReasoningRequest::Medium,
        "high" => BridgeReasoningRequest::High,
        "xhigh" => BridgeReasoningRequest::Xhigh,
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

pub(super) fn apply_reasoning_parameter(
    body: &mut Map<String, Value>,
    model_id: &str,
    selection: ReasoningSelection,
) {
    match selection {
        ReasoningSelection::Toggle(enabled)
            if model_id.starts_with("qwen") || model_id.starts_with("gemma") =>
        {
            body.insert(
                "chat_template_kwargs".to_owned(),
                json!({"enable_thinking": enabled}),
            );
        }
        ReasoningSelection::Effort(effort)
            if model_id.starts_with("deepseek") || model_id.starts_with("glm") =>
        {
            body.insert(
                "reasoning_effort".to_owned(),
                serde_json::to_value(effort).expect("effort serializes"),
            );
        }
        ReasoningSelection::Auto
        | ReasoningSelection::Toggle(_)
        | ReasoningSelection::Effort(_) => {}
    }
}
