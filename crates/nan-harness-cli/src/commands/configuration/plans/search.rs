use super::super::{ConfigurationError, SEARCH_MCP_ID, SEARCH_TOKEN_ENVIRONMENT, json, yaml_quote};
use super::combinators::exclusive_json;
use super::types::{DocumentPlan, JsonPlan, TextBlockPlan};
use std::path::{Path, PathBuf};

pub(crate) fn search_mcp_plan(
    path: PathBuf,
    api_key: &str,
    base_url: &str,
    enabled: bool,
) -> DocumentPlan {
    let entries = enabled
        .then(|| {
            exclusive_json(
                &["mcpServers", SEARCH_MCP_ID],
                json!({
                    "command": "nan-harness",
                    "args": [
                        "__search-mcp",
                        "--provider-base-url",
                        base_url,
                        "--token-env",
                        SEARCH_TOKEN_ENVIRONMENT
                    ],
                    "env": {"NAN_HARNESS_SEARCH_API_KEY": api_key},
                    "enabled": true
                }),
            )
        })
        .into_iter()
        .collect();
    DocumentPlan::Json(JsonPlan { path, entries })
}

pub(crate) fn deepseek_search_plan(
    directory: &Path,
    base_url: &str,
    enabled: bool,
) -> Result<DocumentPlan, ConfigurationError> {
    let body = if enabled {
        Some(format!(
            "- insert:\n    - id: mcp-nan-search\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: nan-search\n        transport: stdio\n        command: nan-harness\n        args: ['__search-mcp', '--provider-base-url', {}, '--token-env', 'NAN_API_KEY']\n        env:\n          NAN_API_KEY: !!js process.env.NAN_API_KEY",
            yaml_quote(base_url)?
        ))
    } else {
        None
    };
    Ok(DocumentPlan::TextBlock(TextBlockPlan {
        path: directory.join("cordis.patch.yml"),
        begin: "# nan-harness:begin search-mcp".to_owned(),
        end: "# nan-harness:end search-mcp".to_owned(),
        body,
        conflicting_keys: vec!["- id: mcp-nan-search".to_owned()],
    }))
}

pub(crate) fn hermes_search_provider() -> String {
    r#"import os
from pathlib import Path

import httpx
import yaml

from agent.web_search_provider import WebSearchProvider


def _connection():
    home = Path(os.environ.get("HERMES_HOME", Path.home() / ".hermes"))
    with (home / "config.yaml").open(encoding="utf-8") as stream:
        model = (yaml.safe_load(stream) or {}).get("model", {})
    return str(model.get("api_key", "")).strip(), str(model.get("base_url", "")).rstrip("/")


class NanHarnessWebSearchProvider(WebSearchProvider):
    @property
    def name(self):
        return "nan-harness"

    @property
    def display_name(self):
        return "nan-search"

    def is_available(self):
        try:
            api_key, base_url = _connection()
            return bool(api_key and base_url)
        except Exception:
            return False

    def search(self, query, limit=5):
        try:
            api_key, base_url = _connection()
            response = httpx.post(
                f"{base_url}/search",
                headers={"Authorization": f"Bearer {api_key}"},
                json={"query": query, "maxResults": min(max(int(limit), 1), 20)},
                timeout=60,
            )
            response.raise_for_status()
            results = response.json().get("results", [])
            return {
                "success": True,
                "data": {
                    "web": [
                        {
                            "title": item.get("title", ""),
                            "url": item.get("url", ""),
                            "description": item.get("snippet", ""),
                            "position": position,
                        }
                        for position, item in enumerate(results, start=1)
                    ]
                },
            }
        except Exception:
            return {"success": False, "error": "NH-SEARCH-HTTP"}
"#
    .to_owned()
}

pub(crate) fn openclaw_search_plugin() -> String {
    r#"import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";

const parameters = {
  type: "object",
  properties: {
    query: { type: "string" },
    count: { type: "integer", minimum: 1, maximum: 20 }
  },
  required: ["query"],
  additionalProperties: false
};

export default definePluginEntry({
  id: "nan-harness-search",
  name: "nan-search",
  description: "nan-search",
  register(api) {
    const connection = () => {
      const provider = api.config?.models?.providers?.nan ?? {};
      return {
        apiKey: typeof provider.apiKey === "string" ? provider.apiKey : "",
        baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl.replace(/\/+$/, "") : ""
      };
    };
    api.registerWebSearchProvider({
      id: "nan-harness",
      label: "nan-search",
      hint: "nan-search",
      requiresCredential: true,
      envVars: [],
      placeholder: "nan-session",
      signupUrl: "https://nan.im",
      credentialPath: "",
      getCredentialValue: () => connection().apiKey,
      setCredentialValue: () => {},
      createTool: () => ({
        description: "nan-search",
        parameters,
        execute: async (args, context) => {
          const query = typeof args.query === "string" ? args.query.trim() : "";
          if (!query) throw new Error("NH-SEARCH-QUERY");
          const count = Number.isInteger(args.count) ? Math.min(Math.max(args.count, 1), 20) : 5;
          const { apiKey, baseUrl } = connection();
          const response = await fetch(`${baseUrl}/search`, {
            method: "POST",
            headers: {
              authorization: `Bearer ${apiKey}`,
              "content-type": "application/json"
            },
            body: JSON.stringify({ query, maxResults: count }),
            signal: context?.signal
          });
          if (!response.ok) throw new Error(`NH-SEARCH-HTTP-${response.status}`);
          const payload = await response.json();
          const results = Array.isArray(payload.results) ? payload.results : [];
          return {
            query,
            provider: "nan-harness",
            count: results.length,
            externalContent: { untrusted: true, source: "web_search", provider: "nan-harness" },
            results
          };
        }
      })
    });
  }
});
"#
    .to_owned()
}
