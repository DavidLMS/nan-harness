use super::{
    CodingModelProfile, ConfigurationError, ConfigurationPaths, DEFAULT_MODEL_ID, HarnessKind,
    ManagedSearchStatus, OMP_SEARCH_EXTENSION_FILE, OmpSearchMode, PI_SEARCH_EXTENSION_FILE,
    PiSearchMode, ReasoningEffort, ReasoningPolicy, SEARCH_MCP_ID, SEARCH_TOKEN_ENVIRONMENT,
    SUPPORTED_HARNESSES, Value, WebSearchPolicy, YamlValue, dotenv_quote, json,
    render_omp_search_extension, render_pi_search_extension, yaml_quote,
};
use std::path::{Path, PathBuf};

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
    pub(crate) value: YamlValue,
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
    pub(crate) value: Value,
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

pub(crate) fn for_harness(
    paths: &ConfigurationPaths,
    harness: HarnessKind,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: ManagedSearchStatus,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let plans = match harness {
        HarnessKind::OpenCode => vec![DocumentPlan::Json(JsonPlan {
            path: paths.opencode_auth_path.clone(),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type": "api", "key": api_key}),
            )],
        })],
        HarnessKind::Pi => pi_family_plans(
            &paths.home_directory.join(".pi/agent"),
            api_key,
            base_url,
            models,
            default_model,
            search,
        ),
        HarnessKind::Omp => omp_plans(
            &paths.omp_directory,
            api_key,
            base_url,
            models,
            default_model,
            search,
        )?,
        HarnessKind::PrimeAgent => pi_family_plans(
            &paths.prime_directory,
            api_key,
            base_url,
            models,
            default_model,
            search,
        ),
        HarnessKind::QwenCode => {
            qwen_plans(&paths.qwen_directory, api_key, base_url, search.managed)
        }
        HarnessKind::DeepSeekHarness => {
            deepseek_plans(&paths.deepseek_directory, api_key, base_url, search.managed)?
        }
        HarnessKind::Aider => vec![DocumentPlan::TextBlock(TextBlockPlan {
            path: paths.home_directory.join(".aider.conf.yml"),
            begin: "# nan-harness:begin provider-defaults".to_owned(),
            end: "# nan-harness:end provider-defaults".to_owned(),
            body: Some(format!(
                "api-key:\n  - {}\nmodel: {}",
                yaml_quote(&format!("nan={api_key}"))?,
                yaml_quote(&format!("nan/{default_model}"))?
            )),
            conflicting_keys: vec!["api-key:".to_owned(), "model:".to_owned()],
        })],
        HarnessKind::Hermes => hermes_plans(
            &paths.home_directory.join(".hermes"),
            api_key,
            base_url,
            default_model,
            search.managed,
        )?,
        HarnessKind::OpenClaw => openclaw_plans(
            &paths.home_directory.join(".openclaw"),
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
        ),
        HarnessKind::Cline => cline_plans(
            &paths.home_directory.join(".cline/data/settings"),
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
        ),
        HarnessKind::KimiCode => vec![
            DocumentPlan::Kimi(KimiPlan {
                path: paths.kimi_directory.join("config.toml"),
                api_key: api_key.to_owned(),
                base_url: base_url.to_owned(),
                models: models.to_vec(),
                default_model: default_model.to_owned(),
            }),
            search_mcp_plan(
                paths.kimi_directory.join("mcp.json"),
                api_key,
                base_url,
                search.managed,
            ),
        ],
        HarnessKind::Goose => goose_plans(
            &paths.goose_directory,
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
        )?,
        HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => {
            return Err(ConfigurationError::BridgeOnly(harness));
        }
    };
    Ok(plans)
}

pub(crate) fn pi_family_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: ManagedSearchStatus,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("models.json"),
            entries: vec![exclusive_json(
                &["providers", "nan"],
                pi_provider(base_url, models),
            )],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("auth.json"),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type": "api_key", "key": api_key}),
            )],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("settings.json"),
            entries: vec![
                override_json(&["defaultProvider"], Value::String("nan".to_owned())),
                override_json(&["defaultModel"], Value::String(default_model.to_owned())),
            ],
        }),
        search_mcp_plan(directory.join("mcp.json"), api_key, base_url, false),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join(PI_SEARCH_EXTENSION_FILE),
            payload: search.managed.then(|| {
                render_pi_search_extension(
                    base_url,
                    if search.policy == WebSearchPolicy::Force {
                        PiSearchMode::Force
                    } else {
                        PiSearchMode::Auto
                    },
                )
                .into_bytes()
            }),
        }),
    ]
}

pub(crate) fn omp_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search: ManagedSearchStatus,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let models_path = preferred_yaml_path(directory, "models.yml", "models.yaml");
    let config_path = preferred_yaml_path(directory, "config.yml", "config.yaml");
    Ok(vec![
        DocumentPlan::Yaml(YamlPlan {
            path: models_path,
            entries: vec![YamlEntryPlan {
                path: vec!["providers".to_owned(), "nan".to_owned()],
                value: to_yaml_value(omp_provider(api_key, base_url, models))?,
                mode: YamlEntryMode::Exclusive,
            }],
            legacy_block: None,
        }),
        DocumentPlan::Yaml(YamlPlan {
            path: config_path,
            entries: [
                "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
                "advisor",
            ]
            .into_iter()
            .map(|role| YamlEntryPlan {
                path: vec!["modelRoles".to_owned(), role.to_owned()],
                value: YamlValue::String(format!("nan/{default_model}")),
                mode: YamlEntryMode::Override,
            })
            .collect(),
            legacy_block: None,
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join(OMP_SEARCH_EXTENSION_FILE),
            payload: search.managed.then(|| {
                render_omp_search_extension(
                    base_url,
                    if search.policy == WebSearchPolicy::Force {
                        OmpSearchMode::Force
                    } else {
                        OmpSearchMode::Auto
                    },
                )
                .into_bytes()
            }),
        }),
    ])
}

pub(crate) fn preferred_yaml_path(directory: &Path, canonical: &str, compatible: &str) -> PathBuf {
    let canonical = directory.join(canonical);
    let compatible = directory.join(compatible);
    if !canonical.exists() && compatible.exists() {
        compatible
    } else {
        canonical
    }
}

pub(crate) fn cline_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("providers.json"),
            entries: vec![
                exclusive_json(
                    &["providers", "openai-compatible"],
                    json!({
                        "settings": {
                            "apiKey": api_key,
                            "baseUrl": base_url,
                            "model": default_model,
                            "provider": "openai-compatible"
                        },
                        "tokenSource": "manual",
                        "updatedAt": "1970-01-01T00:00:00.000Z"
                    }),
                ),
                override_json(
                    &["lastUsedProvider"],
                    Value::String("openai-compatible".to_owned()),
                ),
                override_json(&["version"], json!(1)),
            ],
        }),
        DocumentPlan::Json(JsonPlan {
            path: directory.join("models.json"),
            entries: vec![
                exclusive_json(
                    &["providers", "openai-compatible", "models"],
                    cline_models(models),
                ),
                override_json(&["version"], json!(1)),
            ],
        }),
        search_mcp_plan(
            directory.join("mcp_settings.json"),
            api_key,
            base_url,
            search_managed,
        ),
    ]
}

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

pub(crate) fn qwen_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    vec![
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join(".env"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_API_KEY={}", dotenv_quote(api_key))),
            conflicting_keys: vec!["NAN_API_KEY=".to_owned()],
        }),
        search_mcp_plan(
            directory.join("mcp.json"),
            api_key,
            base_url,
            search_managed,
        ),
    ]
}

pub(crate) fn deepseek_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    search_managed: bool,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    Ok(vec![
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join(".credentials.yaml"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_API_KEY: {}", yaml_quote(api_key)?)),
            conflicting_keys: vec!["NAN_API_KEY:".to_owned()],
        }),
        deepseek_search_plan(directory, base_url, search_managed)?,
    ])
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

pub(crate) fn hermes_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    default_model: &str,
    search_managed: bool,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let mut entries = vec![YamlEntryPlan {
        path: vec!["model".to_owned()],
        value: to_yaml_value(json!({
            "default": default_model,
            "provider": "custom",
            "base_url": base_url,
            "api_key": api_key
        }))?,
        mode: YamlEntryMode::Exclusive,
    }];
    if search_managed {
        entries.extend([
            YamlEntryPlan {
                path: vec!["plugins".to_owned(), "enabled".to_owned()],
                value: YamlValue::String("web/nan_harness".to_owned()),
                mode: YamlEntryMode::AppendUnique,
            },
            YamlEntryPlan {
                path: vec!["web".to_owned(), "search_backend".to_owned()],
                value: YamlValue::String("nan-harness".to_owned()),
                mode: YamlEntryMode::Override,
            },
        ]);
    }
    Ok(vec![
        DocumentPlan::Yaml(YamlPlan {
            path: directory.join("config.yaml"),
            entries,
            legacy_block: Some(LegacyTextBlock {
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
            }),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/__init__.py"),
            payload: search_managed.then(|| b"from .provider import NanHarnessWebSearchProvider\n\n\ndef register(ctx):\n    ctx.register_web_search_provider(NanHarnessWebSearchProvider())\n".to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/provider.py"),
            payload: search_managed.then(|| hermes_search_provider().into_bytes()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("plugins/web/nan_harness/plugin.yaml"),
            payload: search_managed.then(|| b"name: nan-search\nkind: backend\nversion: 1.0.0\ndescription: nan-search\nauthor: NaN\nprovides_web_providers:\n  - nan-harness\n".to_vec()),
        }),
    ])
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

pub(crate) fn openclaw_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Vec<DocumentPlan> {
    let plugin_directory = directory.join("extensions/nan-harness-search");
    let mut entries = vec![
        exclusive_json(
            &["models", "providers", "nan"],
            openclaw_provider(api_key, base_url, models),
        ),
        override_json(
            &["agents", "defaults", "model", "primary"],
            Value::String(format!("nan/{default_model}")),
        ),
        override_json(&["agents", "defaults", "models"], openclaw_aliases(models)),
        override_json(&["models", "mode"], Value::String("merge".to_owned())),
    ];
    if search_managed {
        entries.extend([
            append_unique_json(
                &["plugins", "load", "paths"],
                Value::String(plugin_directory.to_string_lossy().into_owned()),
            ),
            exclusive_json(
                &["plugins", "entries", "nan-harness-search"],
                json!({"enabled": true}),
            ),
            override_json(&["tools", "web", "search", "enabled"], Value::Bool(true)),
            override_json(
                &["tools", "web", "search", "provider"],
                Value::String("nan-harness".to_owned()),
            ),
        ]);
    }
    vec![
        DocumentPlan::Json(JsonPlan {
            path: directory.join("openclaw.json"),
            entries,
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("package.json"),
            payload: search_managed.then(|| br#"{"name":"nan-harness-search","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("openclaw.plugin.json"),
            payload: search_managed.then(|| br#"{"id":"nan-harness-search","activation":{"onStartup":false},"contracts":{"webSearchProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#.to_vec()),
        }),
        DocumentPlan::ExactFile(ExactFilePlan {
            path: plugin_directory.join("index.js"),
            payload: search_managed.then(|| openclaw_search_plugin().into_bytes()),
        }),
    ]
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

pub(crate) fn goose_plans(
    directory: &Path,
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
    default_model: &str,
    search_managed: bool,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let provider = serde_json::to_vec_pretty(&json!({
        "name": "nan_harness",
        "engine": "openai",
        "display_name": "NaN",
        "description": "NaN models configured by nan-harness",
        "api_key_env": "NAN_HARNESS_API_KEY",
        "base_url": endpoint,
        "models": models.iter().map(|model| json!({
            "name": model.id,
            "context_limit": model.context_window
        })).collect::<Vec<_>>(),
        "supports_streaming": true,
        "requires_auth": true
    }))
    .map_err(ConfigurationError::SerializeDocument)?;
    Ok(vec![
        DocumentPlan::ExactFile(ExactFilePlan {
            path: directory.join("custom_providers/nan_harness.json"),
            payload: Some(provider),
        }),
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join("secrets.yaml"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!("NAN_HARNESS_API_KEY: {}", yaml_quote(api_key)?)),
            conflicting_keys: vec!["NAN_HARNESS_API_KEY:".to_owned()],
        }),
        DocumentPlan::Yaml(YamlPlan {
            path: directory.join("config.yaml"),
            entries: goose_config_entries(api_key, base_url, default_model, search_managed)?,
            legacy_block: Some(LegacyTextBlock {
                begin: "# nan-harness:begin provider-defaults".to_owned(),
                end: "# nan-harness:end provider-defaults".to_owned(),
            }),
        }),
    ])
}

pub(crate) fn goose_config_entries(
    _api_key: &str,
    base_url: &str,
    default_model: &str,
    search_managed: bool,
) -> Result<Vec<YamlEntryPlan>, ConfigurationError> {
    let mut entries = vec![
        YamlEntryPlan {
            path: vec!["GOOSE_PROVIDER".to_owned()],
            value: YamlValue::String("nan_harness".to_owned()),
            mode: YamlEntryMode::Exclusive,
        },
        YamlEntryPlan {
            path: vec!["GOOSE_MODEL".to_owned()],
            value: YamlValue::String(default_model.to_owned()),
            mode: YamlEntryMode::Exclusive,
        },
    ];
    if search_managed {
        entries.push(YamlEntryPlan {
            path: vec!["extensions".to_owned(), SEARCH_MCP_ID.to_owned()],
            value: to_yaml_value(json!({
                "name": SEARCH_MCP_ID,
                "type": "stdio",
                "cmd": "nan-harness",
                "args": [
                    "__search-mcp",
                    "--provider-base-url",
                    base_url,
                    "--token-env",
                    "NAN_HARNESS_API_KEY"
                ],
                "env_keys": ["NAN_HARNESS_API_KEY"],
                "enabled": true,
                "timeout": 60
            }))?,
            mode: YamlEntryMode::Exclusive,
        });
    }
    Ok(entries)
}

pub(crate) fn to_yaml_value(value: Value) -> Result<YamlValue, ConfigurationError> {
    serde_yaml_ng::to_value(value).map_err(ConfigurationError::SerializeYaml)
}

pub(crate) fn pi_provider(base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": "NAN_API_KEY",
        "models": models.iter().map(pi_model).collect::<Vec<_>>()
    })
}

pub(crate) fn pi_model(model: &CodingModelProfile) -> Value {
    json!({
        "id": model.id,
        "name": model.display_name,
        "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
        "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window,
        "maxTokens": model.max_output_tokens,
        "compat": {
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": matches!(model.reasoning, ReasoningPolicy::Effort { .. }),
            "maxTokensField": "max_tokens"
        }
    })
}

pub(crate) fn omp_provider(api_key: &str, base_url: &str, models: &[CodingModelProfile]) -> Value {
    json!({
        "baseUrl": base_url,
        "api": "openai-completions",
        "apiKey": api_key,
        "authHeader": true,
        "models": models.iter().map(omp_model).collect::<Vec<_>>()
    })
}

pub(crate) fn omp_model(model: &CodingModelProfile) -> Value {
    let mut value = pi_model(model);
    if let ReasoningPolicy::Effort { supported, default } = model.reasoning {
        let supported = supported
            .into_iter()
            .map(|effort| Value::String(reasoning_effort_name(effort).to_owned()))
            .collect::<Vec<_>>();
        let default = Value::String(reasoning_effort_name(default).to_owned());
        let effort_map = Value::Object(
            supported
                .iter()
                .filter_map(|effort| {
                    effort
                        .as_str()
                        .map(|name| (name.to_owned(), Value::String(name.to_owned())))
                })
                .collect(),
        );
        value["thinking"] = json!({
            "mode": "effort",
            "efforts": supported,
            "defaultLevel": default,
            "effortMap": effort_map.clone()
        });
        value["compat"]["reasoningEffortMap"] = effort_map;
    }
    value
}

const fn reasoning_effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
}

pub(crate) fn openclaw_provider(
    api_key: &str,
    base_url: &str,
    models: &[CodingModelProfile],
) -> Value {
    json!({
        "api": "openai-completions",
        "apiKey": api_key,
        "baseUrl": base_url,
        "models": models.iter().map(|model| json!({
            "id": model.id,
            "name": model.display_name,
            "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
            "input": if model.image_input { vec!["text", "image"] } else { vec!["text"] },
            "contextWindow": model.context_window,
            "maxTokens": model.max_output_tokens
        })).collect::<Vec<_>>()
    })
}

pub(crate) fn openclaw_aliases(models: &[CodingModelProfile]) -> Value {
    Value::Object(
        models
            .iter()
            .map(|model| (format!("nan/{}", model.id), json!({})))
            .collect(),
    )
}

pub(crate) fn cline_models(models: &[CodingModelProfile]) -> Value {
    Value::Array(
        models
            .iter()
            .map(|model| {
                json!({
                    "id": model.id,
                    "name": model.display_name,
                    "contextWindow": model.context_window,
                    "maxTokens": model.max_output_tokens,
                    "supportsImages": model.image_input,
                    "supportsReasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown)
                })
            })
            .collect(),
    )
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
