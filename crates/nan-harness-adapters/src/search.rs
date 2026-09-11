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
const NAN_SEARCH_HELPER_TIMEOUT_MS = 95_000;
const NAN_SEARCH_MAX_MESSAGE_BYTES = 1024 * 1024;

function stopNanSearchHelper() {
  const client = nanSearchClient;
  nanSearchClient = undefined;
  client?.stop();
}

process.once("exit", stopNanSearchHelper);

function nanSearchHelper() {
  if (nanSearchClient) return nanSearchClient;
  const child = spawn(process.env.NAN_HARNESS_BIN || "nanh", ["__search-mcp"], {
    stdio: ["pipe", "pipe", "ignore"]
  });
  child.stdout.setEncoding("utf8");
  let buffer = "";
  let closed = false;
  const pending = new Map();
  function keepAlive(active) {
    const method = active ? "ref" : "unref";
    child[method]();
    child.stdin[method]?.();
    child.stdout[method]?.();
  }
  function fail() {
    if (closed) return;
    closed = true;
    if (nanSearchClient?.child === child) nanSearchClient = undefined;
    for (const request of pending.values()) request.finish(new Error("NH-SEARCH-HELPER"));
    child.kill();
    child.stdin.destroy();
    child.stdout.destroy();
    keepAlive(false);
  }
  child.once("error", fail);
  child.once("exit", fail);
  child.stdin.on("error", fail);
  child.stdout.on("error", fail);
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    if (Buffer.byteLength(buffer) > NAN_SEARCH_MAX_MESSAGE_BYTES) return fail();
    let newline;
    while ((newline = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (!line.trim()) continue;
      let response;
      try { response = JSON.parse(line); } catch { return fail(); }
      if (!response || response.jsonrpc !== "2.0") return fail();
      pending.get(response.id)?.finish(undefined, response);
    }
  });
  const client = {
    child,
    stop: fail,
    request(method, params, signal) {
      if (closed || pending.size >= 32) return Promise.reject(new Error("NH-SEARCH-HELPER"));
      if (signal?.aborted) return Promise.reject(new Error("NH-SEARCH-ABORTED"));
      const id = ++nanSearchRequestId;
      const payload = `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`;
      if (Buffer.byteLength(payload) > NAN_SEARCH_MAX_MESSAGE_BYTES) return Promise.reject(new Error("NH-SEARCH-QUERY"));
      return new Promise((resolve, reject) => {
        const abort = () => finish(new Error("NH-SEARCH-ABORTED"));
        const timer = setTimeout(fail, NAN_SEARCH_HELPER_TIMEOUT_MS);
        function finish(error, response) {
          if (!pending.delete(id)) return;
          clearTimeout(timer);
          signal?.removeEventListener("abort", abort);
          if (pending.size === 0) keepAlive(false);
          if (error) reject(error); else resolve(response);
        }
        pending.set(id, { finish });
        keepAlive(true);
        signal?.addEventListener("abort", abort, { once: true });
        child.stdin.write(payload, (error) => { if (error) fail(); });
      });
    }
  };
  nanSearchClient = client;
  return client;
}

async function nanSearchMcp(params, signal) {
  if (signal?.aborted) throw new Error("NH-SEARCH-ABORTED");
  const client = nanSearchHelper();
  // The bundled helper accepts tools/call directly. Avoid a detached initialization
  // promise so startup failures are always observed by the requesting tool.
  const response = await client.request("tools/call", {
    name: "web_search",
    arguments: {
      query: params.query,
      max_results: params.maxResults,
      allowed_domains: params.allowedDomains ?? [],
      blocked_domains: params.blockedDomains ?? []
    }
  }, signal);
  const result = response.result;
  if (response.error || result?.isError) {
    const message = result?.content?.find((item) => item.type === "text")?.text;
    throw new Error(message || "NH-SEARCH-HELPER");
  }
  if (!Array.isArray(result?.structuredContent?.results)) throw new Error("NH-SEARCH-HELPER");
  return result.structuredContent.results;
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
  if (!query || Buffer.byteLength(query) > 8 * 1024) throw new Error("NH-SEARCH-QUERY");

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
