use super::chunk::{Chunk, ToolCallDelta};
use crate::usage::UsageValues;
use std::collections::{BTreeMap, btree_map::Entry};

#[derive(Debug)]
pub(super) struct ToolState {
    content_index: usize,
    id: String,
    name: String,
    pending_arguments: String,
    started: bool,
}

impl ToolState {
    fn new(content_index: usize) -> Self {
        Self {
            content_index,
            id: String::new(),
            name: String::new(),
            pending_arguments: String::new(),
            started: false,
        }
    }

    fn apply(&mut self, delta: ToolCallDelta) {
        if let Some(id) = delta.id {
            self.id.push_str(&id);
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                self.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                self.pending_arguments.push_str(&arguments);
            }
        }
    }

    pub(super) const fn content_index(&self) -> usize {
        self.content_index
    }

    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) const fn started(&self) -> bool {
        self.started
    }

    pub(super) fn ready_to_start(&self) -> bool {
        !self.id.is_empty() && !self.name.is_empty()
    }

    pub(super) fn mark_started(&mut self) {
        self.started = true;
    }

    pub(super) fn take_pending_arguments(&mut self) -> String {
        std::mem::take(&mut self.pending_arguments)
    }
}

#[derive(Debug, Default)]
pub(super) struct StreamState {
    started: bool,
    text_index: Option<usize>,
    thinking_index: Option<usize>,
    tools: BTreeMap<usize, ToolState>,
    next_content_index: usize,
    message_id: Option<String>,
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    usage: Option<UsageValues>,
}

impl StreamState {
    pub(super) fn update_metadata(&mut self, chunk: &Chunk) {
        if self.message_id.is_none() {
            self.message_id.clone_from(&chunk.id);
        }
        if let Some(usage) = &chunk.usage {
            self.input_tokens = usage.prompt_tokens;
            self.output_tokens = usage.completion_tokens;
            self.usage = Some(UsageValues {
                input: usage.prompt_tokens,
                output: usage.completion_tokens,
                reasoning: usage
                    .completion_tokens_details
                    .as_ref()
                    .map_or(0, |details| details.reasoning_tokens),
            });
        }
    }

    pub(super) const fn started(&self) -> bool {
        self.started
    }

    pub(super) fn mark_started(&mut self) {
        self.started = true;
    }

    pub(super) fn thinking_content_index(&mut self) -> (usize, bool) {
        if let Some(index) = self.thinking_index {
            (index, false)
        } else {
            let index = self.reserve_content_index();
            self.thinking_index = Some(index);
            (index, true)
        }
    }

    pub(super) fn text_content_index(&mut self) -> (usize, bool) {
        if let Some(index) = self.text_index {
            (index, false)
        } else {
            let index = self.reserve_content_index();
            self.text_index = Some(index);
            (index, true)
        }
    }

    pub(super) fn apply_tool_delta(&mut self, delta: ToolCallDelta) -> &mut ToolState {
        let tool = match self.tools.entry(delta.index) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let content_index = self.next_content_index;
                self.next_content_index += 1;
                entry.insert(ToolState::new(content_index))
            }
        };
        tool.apply(delta);
        tool
    }

    pub(super) fn update_finish_reason(&mut self, finish_reason: Option<String>) {
        if finish_reason.is_some() {
            self.finish_reason = finish_reason;
        }
    }

    pub(super) fn unfinished_tool(&self) -> Option<&ToolState> {
        self.tools.values().find(|tool| !tool.started())
    }

    pub(super) fn content_indexes(&self) -> Vec<usize> {
        let mut indexes = self
            .thinking_index
            .into_iter()
            .chain(self.text_index)
            .chain(self.tools.values().map(ToolState::content_index))
            .collect::<Vec<_>>();
        indexes.sort_unstable();
        indexes
    }

    pub(super) fn message_id(&self) -> &str {
        self.message_id.as_deref().unwrap_or("msg_nan_harness")
    }

    pub(super) fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    pub(super) fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    pub(super) const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    pub(super) const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    pub(super) const fn usage(&self) -> Option<UsageValues> {
        self.usage
    }

    fn reserve_content_index(&mut self) -> usize {
        let index = self.next_content_index;
        self.next_content_index += 1;
        index
    }
}
