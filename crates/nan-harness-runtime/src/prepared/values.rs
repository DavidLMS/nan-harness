use crate::temporary::TemporaryWorkspace;
use nan_harness_core::CodingModelProfile;
use nan_harness_core::launch_plan::{
    ARTIFACT_PLACEHOLDER_PREFIX, BRIDGE_BASE_URL_PLACEHOLDER, FX_GATEWAY_CHAT_URL_PLACEHOLDER,
    GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
    PROVIDER_BASE_URL_PLACEHOLDER, USER_HOME_PLACEHOLDER,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{PreparedError, catalogs};

pub(super) struct RuntimeRenderValues<'a> {
    pub(super) provider_base_url: &'a str,
    pub(super) bridge_base_url: Option<&'a str>,
    pub(super) bridge_chat_url: Option<&'a str>,
    pub(super) selected_reasoning_effort: Option<&'a str>,
    pub(super) web_search_enabled: bool,
}

pub(super) fn render_public_value(
    value: &str,
    runtime_values: &RuntimeRenderValues<'_>,
    user_home: &Path,
    selected_model_id: &str,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, PreparedError> {
    let value = value.replace(USER_HOME_PLACEHOLDER, &user_home.to_string_lossy());
    let value = catalogs::render_model_catalogs(
        &value,
        runtime_values.provider_base_url,
        selected_model_id,
        model_catalog,
    )
    .map_err(PreparedError::ModelCatalog)?;
    let value = merge_goose_additional_config_files(&value)?;
    render_runtime_value(&value, runtime_values)
}

fn merge_goose_additional_config_files(value: &str) -> Result<String, PreparedError> {
    let Some(temporary_path) = value.strip_prefix(GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER) else {
        return Ok(value.to_owned());
    };
    join_goose_config_paths(
        std::env::var_os("GOOSE_ADDITIONAL_CONFIG_FILES").as_deref(),
        temporary_path,
    )
}

pub(super) fn join_goose_config_paths(
    existing: Option<&OsStr>,
    temporary_path: &str,
) -> Result<String, PreparedError> {
    if temporary_path.is_empty() {
        return Err(PreparedError::InvalidEnvironmentPathList);
    }
    let mut paths = existing
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    paths.push(PathBuf::from(temporary_path));
    std::env::join_paths(paths)
        .map_err(|_| PreparedError::InvalidEnvironmentPathList)?
        .into_string()
        .map_err(|_| PreparedError::InvalidEnvironmentPathList)
}

pub(super) fn render_runtime_value(
    value: &str,
    runtime_values: &RuntimeRenderValues<'_>,
) -> Result<String, PreparedError> {
    let value = render_nan_search_blocks(value, runtime_values.web_search_enabled)
        .map_err(PreparedError::UnresolvedPlaceholder)?;
    let mut rendered =
        catalogs::render_reasoning_effort(&value, runtime_values.selected_reasoning_effort)
            .map_err(PreparedError::ModelCatalog)?
            .replace(
                PROVIDER_BASE_URL_PLACEHOLDER,
                runtime_values.provider_base_url,
            );
    if rendered.contains(BRIDGE_BASE_URL_PLACEHOLDER) {
        let bridge_base_url = runtime_values.bridge_base_url.ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(BRIDGE_BASE_URL_PLACEHOLDER.to_owned())
        })?;
        rendered = rendered.replace(BRIDGE_BASE_URL_PLACEHOLDER, bridge_base_url);
    }
    if rendered.contains(FX_GATEWAY_CHAT_URL_PLACEHOLDER) {
        let bridge_chat_url = runtime_values.bridge_chat_url.ok_or_else(|| {
            PreparedError::UnresolvedPlaceholder(FX_GATEWAY_CHAT_URL_PLACEHOLDER.to_owned())
        })?;
        rendered = rendered.replace(FX_GATEWAY_CHAT_URL_PLACEHOLDER, bridge_chat_url);
    }
    if rendered.contains("{runtime:") || rendered.contains("{secret:") {
        Err(PreparedError::UnresolvedPlaceholder(rendered))
    } else {
        Ok(rendered)
    }
}

pub(super) fn render_nan_search_blocks(value: &str, enabled: bool) -> Result<String, String> {
    let mut rendered = String::with_capacity(value.len());
    let mut remainder = value;
    loop {
        let Some(begin) = remainder.find(NAN_SEARCH_BLOCK_BEGIN) else {
            if remainder.contains(NAN_SEARCH_BLOCK_END) {
                return Err("malformed NaN search block".to_owned());
            }
            rendered.push_str(remainder);
            return Ok(rendered);
        };
        rendered.push_str(&remainder[..begin]);
        let content = &remainder[begin + NAN_SEARCH_BLOCK_BEGIN.len()..];
        let Some(end) = content.find(NAN_SEARCH_BLOCK_END) else {
            return Err("malformed NaN search block".to_owned());
        };
        let block = &content[..end];
        if block.contains(NAN_SEARCH_BLOCK_BEGIN) {
            return Err("nested NaN search block".to_owned());
        }
        if enabled {
            rendered.push_str(block);
        }
        remainder = &content[end + NAN_SEARCH_BLOCK_END.len()..];
    }
}

pub(super) fn resolve_argument(
    argument: &str,
    workspace: &TemporaryWorkspace,
) -> Result<String, PreparedError> {
    let mut rendered = argument.to_owned();
    while let Some(start) = rendered.find(ARTIFACT_PLACEHOLDER_PREFIX) {
        let content_start = start + ARTIFACT_PLACEHOLDER_PREFIX.len();
        let Some(relative_end) = rendered[content_start..].find('}') else {
            return Err(PreparedError::UnresolvedPlaceholder(rendered));
        };
        let end = content_start + relative_end;
        let artifact_id = &rendered[content_start..end];
        if artifact_id.is_empty() || artifact_id.contains(['{', '}']) {
            return Err(PreparedError::UnresolvedPlaceholder(rendered));
        }
        let path = workspace
            .path(artifact_id)
            .map(path_to_string)
            .ok_or_else(|| PreparedError::UnknownArtifact(artifact_id.to_owned()))?;
        rendered.replace_range(start..=end, &path);
    }
    Ok(rendered)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
