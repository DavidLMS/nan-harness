use super::chunk::{FxToolCallDelta, FxUsage};
use super::completion::FxCompletion;
use super::tools::FxTools;
use crate::usage::UsageValues;

pub(super) struct FxStreamState {
    model_id: String,
    text_started: bool,
    reasoning_started: bool,
    tools: FxTools,
    completion: FxCompletion,
}

impl FxStreamState {
    pub(super) fn new(model_id: String) -> Self {
        Self {
            model_id,
            text_started: false,
            reasoning_started: false,
            tools: FxTools::default(),
            completion: FxCompletion::default(),
        }
    }

    pub(super) fn mark_text_started(&mut self) {
        self.text_started = true;
    }

    pub(super) fn mark_reasoning_started(&mut self) {
        self.reasoning_started = true;
    }

    pub(super) fn update_tool(&mut self, call: FxToolCallDelta) {
        self.tools.update(call);
    }

    pub(super) fn update_finish_reason(&mut self, finish_reason: Option<String>) {
        self.completion.update_finish_reason(finish_reason);
    }

    pub(super) fn update_usage(&mut self, usage: FxUsage) {
        self.completion.update_usage(usage);
    }

    pub(super) fn model_id(&self) -> &str {
        &self.model_id
    }

    pub(super) const fn text_started(&self) -> bool {
        self.text_started
    }

    pub(super) const fn reasoning_started(&self) -> bool {
        self.reasoning_started
    }

    pub(super) const fn tools(&self) -> &FxTools {
        &self.tools
    }

    pub(super) const fn completion(&self) -> &FxCompletion {
        &self.completion
    }

    pub(super) const fn usage(&self) -> Option<UsageValues> {
        self.completion.usage()
    }
}
