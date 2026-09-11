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

const NATIVE_SEARCH_MCP_CLIENT: &str = r#"let nanSearchClient;
let nanSearchRequestId = 0;

function nanSearchHelper() {
  if (nanSearchClient) return nanSearchClient;
  const child = spawn(process.env.NAN_HARNESS_BIN || "nanh", ["__search-mcp"], {
    stdio: ["pipe", "pipe", "ignore"]
  });
  child.stdout.setEncoding("utf8");
  let buffer = "";
  const pending = new Map();
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let newline;
    while ((newline = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (!line.trim()) continue;
      let response;
      try { response = JSON.parse(line); } catch { continue; }
      const request = pending.get(response.id);
      if (!request) continue;
      pending.delete(response.id);
      request.resolve(response);
    }
  });
  const fail = () => {
    for (const request of pending.values()) request.reject(new Error("NH-SEARCH-HELPER"));
    pending.clear();
    if (nanSearchClient?.child === child) nanSearchClient = undefined;
  };
  child.once("error", fail);
  child.once("exit", fail);
  const client = {
    child,
    request(method, params) {
      const id = ++nanSearchRequestId;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`, (error) => {
          if (!error) return;
          pending.delete(id);
          reject(error);
        });
      });
    }
  };
  nanSearchClient = client;
  client.ready = client.request("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: "nan-harness-native-search", version: "1" }
  });
  return client;
}

function stopNanSearchHelper() {
  const child = nanSearchClient?.child;
  nanSearchClient = undefined;
  if (!child || child.exitCode !== null) return;
  child.kill();
}

process.once("exit", stopNanSearchHelper);

function nanSearchAbortable(request, signal) {
  if (!signal) return request;
  if (signal.aborted) return Promise.reject(new Error("NH-SEARCH-ABORTED"));
  return new Promise((resolve, reject) => {
    const abort = () => reject(new Error("NH-SEARCH-ABORTED"));
    signal.addEventListener("abort", abort, { once: true });
    request.then(
      (value) => { signal.removeEventListener("abort", abort); resolve(value); },
      (error) => { signal.removeEventListener("abort", abort); reject(error); }
    );
  });
}

async function nanSearchMcp(params, signal) {
  const client = nanSearchHelper();
  const request = (async () => {
    await client.ready;
    return client.request("tools/call", {
      name: "web_search",
      arguments: {
        query: params.query,
        max_results: Number.isInteger(params.maxResults)
          ? params.maxResults
          : Number.isInteger(params.limit)
            ? params.limit
            : Number.isInteger(params.count)
              ? params.count
              : 10,
        allowed_domains: params.allowedDomains ?? [],
        blocked_domains: params.blockedDomains ?? []
      }
    });
  })();
  const response = await nanSearchAbortable(request, signal);
  const result = response.result;
  if (response.error || result?.isError) {
    const message = result?.content?.find((item) => item.type === "text")?.text;
    throw new Error(message || "NH-SEARCH-HELPER");
  }
  return result?.structuredContent?.results ?? [];
}
"#;

const SAVED_SEARCH_JAVASCRIPT: &str = r#"import { readFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";

const NAN_SEARCH_SETUP_GUIDANCE = "NaN web search is not configured; run `nanh search setup`";
const NAN_SEARCH_MAX_RESULTS = 20;
const NAN_SEARCH_MAX_URL_BYTES = 8 * 1024;
const NAN_SEARCH_MAX_TITLE_CHARS = 500;
const NAN_SEARCH_MAX_SNIPPET_CHARS = 2_000;

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
  const input = params ?? {};
  const query = typeof input.query === "string" ? input.query.trim() : "";
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

  const requested = Number.isInteger(input.maxResults)
    ? input.maxResults
    : Number.isInteger(input.limit)
      ? input.limit
      : Number.isInteger(input.count)
        ? input.count
        : 10;
  const maxResults = Math.min(Math.max(requested, 1), NAN_SEARCH_MAX_RESULTS);
  const allowedDomains = nanSearchDomainFilters(input.allowedDomains);
  const blockedDomains = nanSearchDomainFilters(input.blockedDomains);
  const results = await nanSearchMcp({ ...input, query, maxResults }, signal);
  return results
    .filter((item) => typeof item?.title === "string" && typeof item?.url === "string" && item.title.trim() && item.url.trim())
    .map((item) => ({
      title: nanSearchLimitChars(item.title.trim(), NAN_SEARCH_MAX_TITLE_CHARS),
      url: item.url.trim(),
      snippet: nanSearchLimitChars(
        Object.prototype.hasOwnProperty.call(item, "content")
          ? typeof item.content === "string" ? item.content : ""
          : typeof item.snippet === "string" ? item.snippet : "",
        NAN_SEARCH_MAX_SNIPPET_CHARS
      )
    }))
    .filter((result) => {
      if (/\s/.test(result.url) || new TextEncoder().encode(result.url).length > NAN_SEARCH_MAX_URL_BYTES) return false;
      let url;
      try {
        url = new URL(result.url);
      } catch {
        return false;
      }
      if (!["http:", "https:"].includes(url.protocol) || !url.hostname || url.username || url.password) return false;
      const allowed = allowedDomains.length === 0 || allowedDomains.some((domain) => nanSearchMatchesDomain(url, domain));
      const blocked = blockedDomains.some((domain) => nanSearchMatchesDomain(url, domain));
      return allowed && !blocked;
    })
    .slice(0, maxResults);
}

function nanSearchLimitChars(value, maximum) {
  return Array.from(value).slice(0, maximum).join("");
}

function nanSearchDomainFilters(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new Error("NH-SEARCH-DOMAIN");
  return value.map((domain) => {
    if (typeof domain !== "string" || domain.length === 0 || /\s/.test(domain) || domain.includes("//")) {
      throw new Error("NH-SEARCH-DOMAIN");
    }
    const slash = domain.indexOf("/");
    const host = slash === -1 ? domain : domain.slice(0, slash);
    const rawPath = slash === -1 ? null : domain.slice(slash + 1);
    const path = rawPath === null ? null : rawPath.replace(/\/+$/, "");
    if (!host || host.startsWith(".") || /[:@?#]/.test(host) || (rawPath !== null && !path)) {
      throw new Error("NH-SEARCH-DOMAIN");
    }
    if (path !== null && /[?#]/.test(path)) throw new Error("NH-SEARCH-DOMAIN");
    return { host: host.toLowerCase(), path };
  });
}

function nanSearchMatchesDomain(url, domain) {
  const hostname = url.hostname.toLowerCase();
  const hostMatches = hostname === domain.host || hostname.endsWith(`.${domain.host}`);
  if (!hostMatches || domain.path === null) return hostMatches;
  const path = `/${domain.path}`;
  return url.pathname === path || url.pathname.startsWith(`${path}/`);
}
"#;

/// Shared JavaScript support for persistent native search integrations.
///
/// Persistent integrations must read the provider-neutral endpoint saved by
/// `nanh search setup`. They must not reuse the NaN model credential or the
/// launch-only bridge URL for `SearXNG` requests.
pub(crate) fn saved_search_javascript() -> String {
    format!("{NATIVE_SEARCH_MCP_CLIENT}\n{SAVED_SEARCH_JAVASCRIPT}")
}
