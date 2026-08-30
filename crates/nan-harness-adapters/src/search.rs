use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
};
use serde_json::{Value, json};

pub(crate) const NAN_SEARCH_MCP_ID: &str = "nan-search";
pub(crate) const NAN_SEARCH_MCP_BINARY: &str = "nan-harness";

pub(crate) fn nan_search_endpoint() -> String {
    format!("{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search")
}

pub(crate) fn nan_search_mcp_command(token_environment: &str) -> Value {
    json!([
        NAN_SEARCH_MCP_BINARY,
        "__search-mcp",
        "--endpoint",
        nan_search_endpoint(),
        "--token-env",
        token_environment
    ])
}

pub(crate) fn nan_search_mcp_server(token_environment: &str) -> Value {
    json!({
        "command": NAN_SEARCH_MCP_BINARY,
        "args": [
            "__search-mcp",
            "--endpoint",
            nan_search_endpoint(),
            "--token-env",
            token_environment
        ],
        "enabled": true
    })
}

pub(crate) fn nan_search_mcp_overlay(token_environment: &str) -> String {
    let server = nan_search_mcp_server(token_environment);
    format!(
        "{{{NAN_SEARCH_BLOCK_BEGIN}\"mcpServers\":{{\"{NAN_SEARCH_MCP_ID}\":{server}}}{NAN_SEARCH_BLOCK_END}}}"
    )
}
