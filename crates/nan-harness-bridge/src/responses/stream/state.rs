use super::chunk::{Chunk, ToolCallDelta};
use super::tools::ToolState;
use crate::usage::UsageValues;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(super) struct StreamState {
    response_id: Option<String>,
    created: bool,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, ToolState>,
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    usage: Option<UsageValues>,
}

impl StreamState {
    pub(super) fn update_metadata(&mut self, chunk: &Chunk) {
        if self.response_id.is_none() {
            self.response_id.clone_from(&chunk.id);
        }
        if let Some(usage) = &chunk.usage {
            self.input_tokens = usage.prompt_tokens;
            self.output_tokens = usage.completion_tokens;
            self.reasoning_tokens = usage
                .completion_tokens_details
                .as_ref()
                .map_or(0, |details| details.reasoning_tokens);
            self.usage = Some(UsageValues {
                input: self.input_tokens,
                output: self.output_tokens,
                reasoning: self.reasoning_tokens,
            });
        }
    }

    pub(super) fn update_tool(&mut self, delta: ToolCallDelta) {
        self.tools.entry(delta.index).or_default().apply(delta);
    }

    pub(super) fn response_id(&self) -> &str {
        self.response_id.as_deref().unwrap_or("resp_nan_harness")
    }

    pub(super) const fn created(&self) -> bool {
        self.created
    }

    pub(super) fn mark_created(&mut self) {
        self.created = true;
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn append_text(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub(super) fn reasoning(&self) -> &str {
        &self.reasoning
    }

    pub(super) fn append_reasoning(&mut self, reasoning: &str) {
        self.reasoning.push_str(reasoning);
    }

    pub(super) fn tools(&self) -> &BTreeMap<usize, ToolState> {
        &self.tools
    }

    pub(super) fn text_output_index(&self) -> usize {
        usize::from(!self.reasoning.is_empty())
    }

    pub(super) const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    pub(super) const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    pub(super) const fn reasoning_tokens(&self) -> u64 {
        self.reasoning_tokens
    }

    pub(super) const fn usage(&self) -> Option<UsageValues> {
        self.usage
    }
}
