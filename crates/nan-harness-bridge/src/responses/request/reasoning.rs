use super::wire::ResponsesReasoning;
use crate::error::ApiError;
use nan_harness_core::model::{CodingModelProfile, ReasoningHint, ReasoningSelection};

pub(super) fn validate_reasoning(
    request: Option<&ResponsesReasoning>,
    model: &CodingModelProfile,
) -> Result<ReasoningSelection, ApiError> {
    let Some(effort) = request.and_then(|request| request.effort.as_deref()) else {
        return Ok(ReasoningSelection::Auto);
    };
    let policy = model.reasoning;
    if crate::reasoning::adjusted(policy, effort == "none") {
        return Ok(ReasoningSelection::Toggle(true));
    }
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
    policy.resolve_hint(hint).ok_or_else(|| {
        crate::reasoning::mismatch(
            &model.id,
            policy,
            crate::reasoning::diagnostic_effort(effort),
        )
    })
}
