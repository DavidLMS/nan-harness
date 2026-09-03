use super::super::CodingModelProfile;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) enum DocumentPlan {
    Json(JsonPlan),
    Yaml(YamlPlan),
    TextBlock(TextBlockPlan),
    ExactFile(ExactFilePlan),
    Kimi(KimiPlan),
}

#[derive(Debug, Clone)]
pub(crate) struct YamlPlan {
    pub(crate) path: PathBuf,
    pub(crate) entries: Vec<YamlEntryPlan>,
    pub(crate) legacy_block: Option<LegacyTextBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct YamlEntryPlan {
    pub(crate) path: Vec<String>,
    pub(crate) value: super::super::YamlValue,
    pub(crate) mode: YamlEntryMode,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum YamlEntryMode {
    Exclusive,
    Override,
    AppendUnique,
}

#[derive(Debug, Clone)]
pub(crate) struct LegacyTextBlock {
    pub(crate) begin: String,
    pub(crate) end: String,
}

#[derive(Debug, Clone)]
pub(crate) struct JsonPlan {
    pub(crate) path: PathBuf,
    pub(crate) entries: Vec<JsonEntryPlan>,
}

#[derive(Debug, Clone)]
pub(crate) struct JsonEntryPlan {
    pub(crate) path: Vec<String>,
    pub(crate) value: super::super::Value,
    pub(crate) mode: JsonEntryMode,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum JsonEntryMode {
    Exclusive,
    Override,
    AppendUnique,
}

#[derive(Debug, Clone)]
pub(crate) struct TextBlockPlan {
    pub(crate) path: PathBuf,
    pub(crate) begin: String,
    pub(crate) end: String,
    pub(crate) body: Option<String>,
    pub(crate) conflicting_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExactFilePlan {
    pub(crate) path: PathBuf,
    pub(crate) payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct KimiPlan {
    pub(crate) path: PathBuf,
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) models: Vec<CodingModelProfile>,
    pub(crate) default_model: String,
}
