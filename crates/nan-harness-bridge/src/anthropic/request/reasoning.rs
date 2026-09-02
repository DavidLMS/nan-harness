use super::wire::{
    ContentBlock, Message, MessageContent, MessagesRequest, SystemPrompt, ThinkingConfig,
};
use crate::anthropic::auto_mode;
use crate::error::ApiError;
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};
use serde_json::{Map, Value, json};

pub(super) fn translate_thinking(
    thinking: Option<ThinkingConfig>,
    effort: Option<ReasoningEffort>,
    policy: ReasoningPolicy,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let selection = match (thinking, effort) {
        (None, None) => ReasoningSelection::Auto,
        (None | Some(ThinkingConfig::Adaptive), effort) => {
            adaptive_reasoning_selection(policy, effort)
        }
        (Some(ThinkingConfig::Disabled), None) => ReasoningSelection::Toggle(false),
        (Some(ThinkingConfig::Enabled { budget_tokens }), None) => {
            if budget_tokens < 1_024 {
                return Err(ApiError::InvalidRequest(
                    "thinking.budget_tokens must be at least 1024".to_owned(),
                ));
            }
            ReasoningSelection::Toggle(true)
        }
        (Some(ThinkingConfig::Disabled | ThinkingConfig::Enabled { .. }), Some(_)) => {
            return Err(ApiError::InvalidRequest(
                "output_config.effort requires thinking.type 'adaptive'".to_owned(),
            ));
        }
    };
    if selection == ReasoningSelection::Auto {
        return Ok(());
    }
    if !policy.accepts(selection) {
        return Err(ApiError::InvalidRequest(
            "requested thinking configuration is not supported by this model's reasoning policy"
                .to_owned(),
        ));
    }
    match selection {
        ReasoningSelection::Toggle(enabled) => {
            body.insert(
                "chat_template_kwargs".to_owned(),
                json!({"enable_thinking": enabled}),
            );
        }
        ReasoningSelection::Effort(effort) => {
            body.insert(
                "reasoning_effort".to_owned(),
                serde_json::to_value(effort).expect("reasoning effort serializes"),
            );
        }
        ReasoningSelection::Auto => {}
    }
    Ok(())
}

fn adaptive_reasoning_selection(
    policy: ReasoningPolicy,
    effort: Option<ReasoningEffort>,
) -> ReasoningSelection {
    match (policy, effort) {
        (ReasoningPolicy::Effort { .. }, Some(effort)) => ReasoningSelection::Effort(effort),
        (ReasoningPolicy::Toggle { .. } | ReasoningPolicy::AlwaysOn, Some(_)) => {
            ReasoningSelection::Toggle(true)
        }
        (_, None) => policy.default_selection(),
        (ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown, Some(_)) => {
            ReasoningSelection::Auto
        }
    }
}

pub(super) fn classifier_stage(
    request: &MessagesRequest,
) -> Result<Option<auto_mode::ClassifierStage>, ApiError> {
    let has_qualified_policy = auto_mode::policy_markers().into_iter().all(|marker| {
        request
            .system
            .as_ref()
            .is_some_and(|system| system_contains(system, marker))
    });
    let final_message = request.messages.last();
    let stage_marker = if final_message
        .is_some_and(|message| message_contains(message, auto_mode::stage_one_marker()))
    {
        Some(auto_mode::ClassifierStage::One)
    } else if final_message
        .is_some_and(|message| message_contains(message, auto_mode::stage_two_marker()))
    {
        Some(auto_mode::ClassifierStage::Two)
    } else {
        None
    };
    auto_mode::detect(&auto_mode::RequestFingerprint {
        model: &request.model,
        max_tokens: request.max_tokens,
        shape: if request.stream || !request.tools.is_empty() {
            auto_mode::RequestShape::Other
        } else {
            auto_mode::RequestShape::ClassifierCandidate
        },
        policy: if has_qualified_policy {
            auto_mode::PolicyFingerprint::Qualified
        } else {
            auto_mode::PolicyFingerprint::Unknown
        },
        stage_marker,
    })
}

fn system_contains(system: &SystemPrompt, needle: &str) -> bool {
    match system {
        SystemPrompt::Text(text) => text.contains(needle),
        SystemPrompt::Blocks(blocks) => blocks.iter().any(|block| block.text.contains(needle)),
    }
}

fn message_contains(message: &Message, needle: &str) -> bool {
    match &message.content {
        MessageContent::Text(text) => text.contains(needle),
        MessageContent::Blocks(blocks) => blocks.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains(needle),
            ContentBlock::Image { .. }
            | ContentBlock::Thinking { .. }
            | ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Unsupported => false,
        }),
    }
}
