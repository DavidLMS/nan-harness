use super::super::paths::ZedPaths;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use nan_harness_core::{CodingModelProfile, ReasoningPolicy};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub(super) const GATEWAY_URL: &str = "http://127.0.0.1:41234/v1";

pub(super) struct FixturePaths {
    pub(super) root: tempfile::TempDir,
    pub(super) paths: ZedPaths,
}

pub(super) fn fixture_paths() -> FixturePaths {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let paths = ZedPaths::new(
        root.path().join("config/zed/settings.json"),
        root.path().join("state/zed-desktop"),
    )
    .expect("absolute fixture paths should be valid");
    FixturePaths { root, paths }
}

pub(super) fn write_settings(paths: &ZedPaths, contents: &[u8]) {
    fs::create_dir_all(
        paths
            .settings
            .parent()
            .expect("settings parent should exist"),
    )
    .expect("settings parent should be created");
    fs::write(&paths.settings, contents).expect("settings should be written");
}

pub(super) fn generic_model() -> CodingModelProfile {
    model(
        "qwen3.6",
        "NaN Qwen",
        262_144,
        32_768,
        true,
        ReasoningPolicy::Unknown,
    )
}

pub(super) fn model(
    id: &str,
    display_name: &str,
    context_window: u64,
    max_output_tokens: u64,
    image_input: bool,
    reasoning: ReasoningPolicy,
) -> CodingModelProfile {
    let mut profile = CodingModelProfile::generic(id);
    profile.display_name = display_name.to_owned();
    profile.context_window = context_window;
    profile.max_output_tokens = max_output_tokens;
    profile.image_input = image_input;
    profile.reasoning = reasoning;
    profile
}

pub(super) fn parse_jsonc(contents: &[u8]) -> Value {
    jsonc_parser::parse_to_serde_value(
        std::str::from_utf8(contents).expect("fixture should be UTF-8"),
        &ParseOptions::default(),
    )
    .expect("settings should parse")
}

pub(super) fn append_root_property(path: &Path, name: &str, value: CstInputValue) {
    let source = fs::read_to_string(path).expect("settings should be readable");
    let root = CstRootNode::parse(&source, &ParseOptions::default())
        .expect("settings should parse as CST");
    root.object_value()
        .expect("settings root should be an object")
        .append(name, value);
    fs::write(path, root.to_string()).expect("settings should update");
}

pub(super) fn mutate_managed_field(path: &Path, field: &str) {
    let source = fs::read_to_string(path).expect("settings should be readable");
    let root = CstRootNode::parse(&source, &ParseOptions::default())
        .expect("settings should parse as CST");
    let root_object = root.object_value().expect("root should be an object");
    match field {
        "provider" => root_object
            .object_value("language_models")
            .expect("language_models should exist")
            .object_value("openai_compatible")
            .expect("openai_compatible should exist")
            .get("nan")
            .expect("provider should exist")
            .set_value(CstInputValue::Object(vec![])),
        "default_model" => root_object
            .object_value("agent")
            .expect("agent should exist")
            .get("default_model")
            .expect("default should exist")
            .set_value(CstInputValue::Object(vec![
                (
                    "provider".to_owned(),
                    CstInputValue::String("other".to_owned()),
                ),
                (
                    "model".to_owned(),
                    CstInputValue::String("other".to_owned()),
                ),
            ])),
        _ => unreachable!("unknown managed field"),
    }
    fs::write(path, root.to_string()).expect("settings should update");
}
