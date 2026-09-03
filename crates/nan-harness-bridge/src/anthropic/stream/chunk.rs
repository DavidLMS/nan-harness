use crate::error::ApiError;
use crate::stream_common::{StreamChunk, deserialize_error, parse_chunk};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct Chunk {
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) choices: Vec<Choice>,
    #[serde(default)]
    pub(super) usage: Option<Usage>,
    #[serde(default, deserialize_with = "deserialize_error")]
    error: Option<Value>,
}

impl StreamChunk for Chunk {
    fn stream_error(&self) -> Option<&Value> {
        self.error.as_ref()
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct Choice {
    #[serde(default)]
    pub(super) delta: Delta,
    #[serde(default)]
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Delta {
    #[serde(default)]
    pub(super) content: Option<String>,
    #[serde(default)]
    pub(super) reasoning_content: Option<String>,
    #[serde(default)]
    pub(super) tool_calls: Vec<ToolCallDelta>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ToolCallDelta {
    pub(super) index: usize,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FunctionDelta {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) arguments: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Usage {
    #[serde(default)]
    pub(super) prompt_tokens: u64,
    #[serde(default)]
    pub(super) completion_tokens: u64,
    #[serde(default)]
    pub(super) completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CompletionTokenDetails {
    #[serde(default)]
    pub(super) reasoning_tokens: u64,
}

pub(super) fn parse(data: &str) -> Result<Chunk, ApiError> {
    parse_chunk(data)
}
