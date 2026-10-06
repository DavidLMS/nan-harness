use nan_harness_core::{ReasoningPolicy, ReasoningSelection};
use serde_json::{Map, Value, json};

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
