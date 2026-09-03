use super::super::{
    CodingModelProfile, ConfigurationError, DEFAULT_MODEL_ID, HarnessKind, SUPPORTED_HARNESSES,
    Value, YamlValue,
};
use super::types::{JsonEntryMode, JsonEntryPlan};
use std::path::{Path, PathBuf};

pub(crate) fn preferred_yaml_path(directory: &Path, canonical: &str, compatible: &str) -> PathBuf {
    let canonical = directory.join(canonical);
    let compatible = directory.join(compatible);
    if !canonical.exists() && compatible.exists() {
        compatible
    } else {
        canonical
    }
}

pub(crate) fn to_yaml_value(value: Value) -> Result<YamlValue, ConfigurationError> {
    serde_yaml_ng::to_value(value).map_err(ConfigurationError::SerializeYaml)
}

pub(crate) fn preferred_model(models: &[CodingModelProfile]) -> &str {
    models
        .iter()
        .find(|model| model.id == DEFAULT_MODEL_ID)
        .or_else(|| models.first())
        .map_or(DEFAULT_MODEL_ID, |model| model.id.as_str())
}

pub(crate) fn exclusive_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::Exclusive,
    }
}

pub(crate) fn override_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::Override,
    }
}

pub(crate) fn append_unique_json(path: &[&str], value: Value) -> JsonEntryPlan {
    JsonEntryPlan {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        value,
        mode: JsonEntryMode::AppendUnique,
    }
}

pub(crate) fn ensure_supported(harness: HarnessKind) -> Result<(), ConfigurationError> {
    if SUPPORTED_HARNESSES.contains(&harness) {
        Ok(())
    } else {
        Err(ConfigurationError::BridgeOnly(harness))
    }
}
