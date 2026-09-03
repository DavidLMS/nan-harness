use crate::error::ApiError;
use crate::stream_common::{StreamChunk, deserialize_error, parse_chunk};
use serde::Deserialize;
use serde::de::value::MapAccessDeserializer;
use serde::de::{MapAccess, Visitor};
use serde_json::Value;
use std::marker::PhantomData;

#[derive(Debug, Default)]
pub(super) struct FxObject<T>(pub(super) T);

struct FxObjectVisitor<T>(PhantomData<T>);

impl<'de, T> Visitor<'de> for FxObjectVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = FxObject<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        T::deserialize(MapAccessDeserializer::new(map)).map(FxObject)
    }
}

impl<'de, T> Deserialize<'de> for FxObject<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(FxObjectVisitor(PhantomData))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FxChunk {
    #[serde(default)]
    pub(super) choices: Vec<FxObject<FxChoice>>,
    #[serde(default)]
    pub(super) usage: Option<FxObject<FxUsage>>,
    #[serde(default, deserialize_with = "deserialize_error")]
    error: Option<Value>,
}

impl StreamChunk for FxObject<FxChunk> {
    fn stream_error(&self) -> Option<&Value> {
        self.0.error.as_ref()
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FxChoice {
    #[serde(default)]
    pub(super) delta: FxObject<FxDelta>,
    #[serde(default)]
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct FxDelta {
    #[serde(default)]
    pub(super) content: Option<String>,
    #[serde(default)]
    pub(super) reasoning_content: Option<String>,
    #[serde(default)]
    pub(super) tool_calls: Vec<FxObject<FxToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FxToolCallDelta {
    #[serde(default)]
    pub(super) index: usize,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) function: Option<FxObject<FxFunctionDelta>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FxFunctionDelta {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FxUsage {
    #[serde(default)]
    pub(super) prompt_tokens: u64,
    #[serde(default)]
    pub(super) completion_tokens: u64,
    #[serde(default)]
    pub(super) completion_tokens_details: Option<FxObject<FxCompletionTokenDetails>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FxCompletionTokenDetails {
    #[serde(default)]
    pub(super) reasoning_tokens: u64,
}

pub(super) fn parse(data: &str) -> Result<FxObject<FxChunk>, ApiError> {
    parse_chunk(data)
}
