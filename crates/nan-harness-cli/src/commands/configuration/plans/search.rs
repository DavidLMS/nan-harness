use super::super::{
    SEARCH_MCP_ID, json, render_hermes_search_provider, render_openclaw_search_plugin,
};
use super::combinators::exclusive_json;
use super::types::{DocumentPlan, JsonPlan, TextBlockPlan};
use std::path::{Path, PathBuf};

pub(crate) fn search_mcp_plan(path: PathBuf, enabled: bool) -> DocumentPlan {
    let entries = enabled
        .then(|| {
            exclusive_json(
                &["mcpServers", SEARCH_MCP_ID],
                json!({
                    "command": "nan-harness",
                    "args": [
                        "__search-mcp"
                    ],
                    "enabled": true
                }),
            )
        })
        .into_iter()
        .collect();
    DocumentPlan::Json(JsonPlan { path, entries })
}

pub(crate) fn deepseek_search_plan(directory: &Path, enabled: bool) -> DocumentPlan {
    let body = if enabled {
        Some("- insert:\n    - id: mcp-nan-search\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: nan-search\n        transport: stdio\n        command: nan-harness\n        args: ['__search-mcp']"
            .to_owned())
    } else {
        None
    };
    DocumentPlan::TextBlock(TextBlockPlan {
        path: directory.join("cordis.patch.yml"),
        begin: "# nan-harness:begin search-mcp".to_owned(),
        end: "# nan-harness:end search-mcp".to_owned(),
        body,
        conflicting_keys: vec!["- id: mcp-nan-search".to_owned()],
    })
}

pub(crate) fn hermes_search_provider() -> String {
    render_hermes_search_provider()
}

pub(crate) fn openclaw_search_plugin() -> String {
    render_openclaw_search_plugin()
}
