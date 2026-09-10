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

pub(crate) fn nan_search_goose_overlay(token_environment: &str) -> String {
    let extension = json!({
        "name": NAN_SEARCH_MCP_ID,
        "type": "stdio",
        "cmd": NAN_SEARCH_MCP_BINARY,
        "args": [
            "__search-mcp",
            "--endpoint",
            nan_search_endpoint(),
            "--token-env",
            token_environment
        ],
        "enabled": true,
        "timeout": 60
    });
    format!(
        "{{{NAN_SEARCH_BLOCK_BEGIN}\"extensions\":{{\"{NAN_SEARCH_MCP_ID}\":{extension}}}{NAN_SEARCH_BLOCK_END}}}"
    )
}

/// Shared JavaScript support for persistent native search integrations.
///
/// Persistent integrations must read the provider-neutral endpoint saved by
/// `nanh search setup`. They must not reuse the NaN model credential or the
/// launch-only bridge URL for `SearXNG` requests.
pub(crate) fn saved_search_javascript() -> &'static str {
    r#"import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

const NAN_SEARCH_SETUP_GUIDANCE = "NaN web search is not configured; run `nanh search setup`";

function nanHarnessConfigDirectory() {
  if (process.env.NAN_HARNESS_CONFIG_DIR) return process.env.NAN_HARNESS_CONFIG_DIR;
  if (process.platform === "darwin") {
    return join(homedir(), "Library", "Application Support", "nan-harness");
  }
  if (process.platform === "win32") {
    return join(process.env.APPDATA ?? join(homedir(), "AppData", "Roaming"), "nan-harness");
  }
  return join(process.env.XDG_CONFIG_HOME ?? join(homedir(), ".config"), "nan-harness");
}

async function nanSearchResults(params, signal) {
  const query = typeof params.query === "string" ? params.query.trim() : "";
  if (!query) throw new Error("NH-SEARCH-QUERY");

  let config;
  try {
    config = JSON.parse(await readFile(join(nanHarnessConfigDirectory(), "search.json"), "utf8"));
  } catch (error) {
    if (error?.code === "ENOENT") throw new Error(NAN_SEARCH_SETUP_GUIDANCE);
    throw new Error("NH-SEARCH-CONFIG");
  }
  const baseUrl = typeof config?.baseUrl === "string" ? config.baseUrl.replace(/\/+$/, "") : "";
  if (!baseUrl) throw new Error("NH-SEARCH-CONFIG");
  let endpoint;
  try {
    endpoint = new URL(`${baseUrl}/search`);
    if (!["http:", "https:"].includes(endpoint.protocol) || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) {
      throw new Error("unsafe endpoint");
    }
  } catch {
    throw new Error("NH-SEARCH-CONFIG");
  }

  const requested = Number.isInteger(params.maxResults)
    ? params.maxResults
    : Number.isInteger(params.limit)
      ? params.limit
      : Number.isInteger(params.count)
        ? params.count
        : 10;
  const maxResults = Math.min(Math.max(requested, 1), 20);
  endpoint.searchParams.set("q", query);
  endpoint.searchParams.set("format", "json");
  endpoint.searchParams.set("number_of_results", String(maxResults));
  const response = await fetch(endpoint, { method: "GET", signal });
  if (!response.ok) throw new Error(`NH-SEARCH-HTTP-${response.status}`);
  const payload = await response.json();
  const results = Array.isArray(payload.results) ? payload.results : [];
  return results
    .filter((item) => typeof item?.title === "string" && typeof item?.url === "string" && item.title.trim() && item.url.trim())
    .map((item) => ({
      title: item.title.trim(),
      url: item.url.trim(),
      snippet: typeof item.content === "string" ? item.content : typeof item.snippet === "string" ? item.snippet : ""
    }));
}
"#
}
