use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use crate::search::saved_search_javascript;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, ConfigurationOverlay,
    MEDIA_PROVIDER_BASE_URL_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
    OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, MediaSelection, PlanContext, PlanError,
};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "openclaw-config";
const CONFIG_PATH: &str = "{artifact:openclaw-config}/nan-harness.json";
const SEARCH_PLUGIN_PATH: &str = "{artifact:openclaw-config}/plugins/nan-harness-search";
const MEDIA_PLUGIN_PATH: &str = "{artifact:openclaw-config}/plugins/nan-harness-media";
const SEARCH_PLUGIN_PATH_SENTINEL: &str = "zz__NAN_HARNESS_SEARCH_PLUGIN_PATH__";
const SEARCH_PLUGIN_ENTRY_SENTINEL: &str = "zz__NAN_HARNESS_SEARCH_PLUGIN_ENTRY__";
const SEARCH_TOOLS_SENTINEL: &str = "zz__NAN_HARNESS_SEARCH_TOOLS__";

fn openclaw_config(model_id: &str, media: MediaSelection) -> Result<String, PlanError> {
    let model_reference = format!("nan/{model_id}");
    let mut base_value = json!({
        "$include": "./openclaw.json",
        "agents": {
            "defaults": {
                "model": {"primary": model_reference},
                "models": OPENCLAW_MODEL_ALIASES_PLACEHOLDER
            }
        },
        "models": {
            "mode": "merge",
            "providers": {
                "nan": {
                    "api": "openai-completions",
                    "apiKey": {
                        "id": CREDENTIAL_TARGET,
                        "provider": "default",
                        "source": "env"
                    },
                    "baseUrl": PROVIDER_BASE_URL_PLACEHOLDER,
                    "models": OPENCLAW_MODEL_CATALOG_PLACEHOLDER
                }
            }
        }
    });
    if media.tts {
        base_value["tts"] = json!({
            "provider": "nan-harness",
            "providers": {"nan-harness": {"model": "kokoro"}}
        });
    }
    let mut tools = json!({});
    if media.stt {
        tools["media"] = json!({
            "audio": {
                "enabled": true,
                "models": [{"type": "provider", "provider": "nan-harness", "model": "whisper-1"}]
            }
        });
    }
    tools[SEARCH_TOOLS_SENTINEL] = json!({
        "web": {"search": {"enabled": true, "provider": "nan-harness"}}
    });
    base_value["tools"] = tools;
    if media.image {
        base_value["agents"]["defaults"]["mediaModels"] = json!({
            "image": {"primary": "nan-harness/flux-2-klein"}
        });
    }
    let mut plugin_paths = vec![json!(SEARCH_PLUGIN_PATH_SENTINEL)];
    let mut plugin_entries = json!({});
    if media.any() {
        plugin_paths.insert(0, json!(MEDIA_PLUGIN_PATH));
        plugin_entries["nan-harness-media"] = json!({"enabled": true});
    }
    plugin_entries[SEARCH_PLUGIN_ENTRY_SENTINEL] = json!({"enabled": true});
    base_value["plugins"] = json!({
        "load": {"paths": plugin_paths},
        "entries": plugin_entries
    });

    let base =
        serde_json::to_string(&base_value).map_err(|error| openclaw_serialization_error(&error))?;
    Ok(render_openclaw_search_variants(&base, media))
}

fn render_openclaw_search_variants(base: &str, media: MediaSelection) -> String {
    let search_path = if media.any() {
        format!("{NAN_SEARCH_BLOCK_BEGIN},\"{SEARCH_PLUGIN_PATH}\"{NAN_SEARCH_BLOCK_END}")
    } else {
        format!("{NAN_SEARCH_BLOCK_BEGIN}\"{SEARCH_PLUGIN_PATH}\"{NAN_SEARCH_BLOCK_END}")
    };
    let search_entry = if media.any() {
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN},\"nan-harness-search\":{{\"enabled\":true}}{NAN_SEARCH_BLOCK_END}"
        )
    } else {
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN}\"nan-harness-search\":{{\"enabled\":true}}{NAN_SEARCH_BLOCK_END}"
        )
    };
    let search_tools = if media.stt {
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN},\"web\":{{\"search\":{{\"enabled\":true,\"provider\":\"nan-harness\"}}}}{NAN_SEARCH_BLOCK_END}"
        )
    } else {
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN}\"web\":{{\"search\":{{\"enabled\":true,\"provider\":\"nan-harness\"}}}}{NAN_SEARCH_BLOCK_END}"
        )
    };
    let search_path_target = if media.any() {
        format!(",\"{SEARCH_PLUGIN_PATH_SENTINEL}\"")
    } else {
        format!("\"{SEARCH_PLUGIN_PATH_SENTINEL}\"")
    };
    let search_entry_target = if media.any() {
        format!(",\"{SEARCH_PLUGIN_ENTRY_SENTINEL}\":{{\"enabled\":true}}")
    } else {
        format!("\"{SEARCH_PLUGIN_ENTRY_SENTINEL}\":{{\"enabled\":true}}")
    };
    let search_tools_target = if media.stt {
        format!(
            ",\"{SEARCH_TOOLS_SENTINEL}\":{{\"web\":{{\"search\":{{\"enabled\":true,\"provider\":\"nan-harness\"}}}}}}"
        )
    } else {
        format!(
            "\"{SEARCH_TOOLS_SENTINEL}\":{{\"web\":{{\"search\":{{\"enabled\":true,\"provider\":\"nan-harness\"}}}}}}"
        )
    };
    base.replace(&search_path_target, &search_path)
        .replace(&search_entry_target, &search_entry)
        .replace(&search_tools_target, &search_tools)
}

fn openclaw_serialization_error(error: &serde_json::Error) -> PlanError {
    PlanError::InvalidField {
        field: "configurationOverlays.files.contentTemplate",
        message: DiagnosticText::new(|locale| {
            detail_messages::detail_serialize_openclaw_configuration_failed(locale, &(error))
        }),
    }
}

fn search_plugin_files() -> Vec<OverlayFile> {
    vec![
        OverlayFile {
            path: "plugins/nan-harness-search/package.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"name":"nan-harness-search","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-search/openclaw.plugin.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"id":"nan-harness-search","activation":{"onStartup":false},"contracts":{"webSearchProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-search/index.js".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                r#"import {{ definePluginEntry }} from "openclaw/plugin-sdk/plugin-entry";

const parameters = {{
  type: "object",
  properties: {{
    query: {{ type: "string" }},
    count: {{ type: "integer", minimum: 1, maximum: 20 }}
  }},
  required: ["query"],
  additionalProperties: false
}};

const provider = {{
  id: "nan-harness",
  label: "nan-search",
  hint: "nan-search",
  requiresCredential: true,
  envVars: ["NAN_API_KEY"],
  placeholder: "nan-session",
  signupUrl: "https://nan.im",
  credentialPath: "",
  getCredentialValue: () => process.env.NAN_API_KEY,
  setCredentialValue: () => {{}},
  createTool: () => ({{
    description: "nan-search",
    parameters,
    execute: async (args, context) => {{
      const query = typeof args.query === "string" ? args.query.trim() : "";
      if (!query) throw new Error("NH-SEARCH-QUERY");
      const count = Number.isInteger(args.count) ? Math.min(Math.max(args.count, 1), 20) : 5;
      const response = await fetch("{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search", {{
        method: "POST",
        headers: {{
          authorization: `Bearer ${{process.env.NAN_API_KEY ?? ""}}`,
          "content-type": "application/json"
        }},
        body: JSON.stringify({{ query, maxResults: count }}),
        signal: context?.signal
      }});
      if (!response.ok) throw new Error(`NH-SEARCH-HTTP-${{response.status}}`);
      const payload = await response.json();
      const results = Array.isArray(payload.results) ? payload.results : [];
      return {{
        query,
        provider: "nan-harness",
        count: results.length,
        externalContent: {{ untrusted: true, source: "web_search", provider: "nan-harness" }},
        results
      }};
    }}
  }})
}};

export default definePluginEntry({{
  id: "nan-harness-search",
  name: "nan-search",
  description: "nan-search",
  register(api) {{
    api.registerWebSearchProvider(provider);
  }}
}});
"#
            ),
            policy: OverlayFilePolicy::Replace,
        },
    ]
}

fn media_plugin_files(media: MediaSelection) -> Vec<OverlayFile> {
    if !media.any() {
        return Vec::new();
    }
    vec![
        OverlayFile {
            path: "plugins/nan-harness-media/package.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"name":"nan-harness-media","version":"1.0.0","type":"module","peerDependencies":{"openclaw":">=2026.3.24"},"openclaw":{"extensions":["./index.js"]}}"#.to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-media/openclaw.plugin.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: r#"{"id":"nan-harness-media","activation":{"onStartup":false},"contracts":{"speechProviders":["nan-harness"],"mediaUnderstandingProviders":["nan-harness"],"imageGenerationProviders":["nan-harness"]},"configSchema":{"type":"object","additionalProperties":false}}"#.to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/nan-harness-media/index.js".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: render_openclaw_media_plugin(MEDIA_PROVIDER_BASE_URL_PLACEHOLDER),
            policy: OverlayFilePolicy::Replace,
        },
    ]
}

/// Renders the `OpenClaw` provider plugin for a persistent or launch-scoped home.
#[must_use]
pub fn render_openclaw_media_plugin(base_url: &str) -> String {
    let encoded_base_url = serde_json::to_string(base_url).unwrap_or_else(|_| "\"\"".to_owned());
    format!(
        r#"import {{ definePluginEntry }} from "openclaw/plugin-sdk/plugin-entry";
import {{ promises as fs }} from "node:fs";
import {{ tmpdir }} from "node:os";
import {{ join }} from "node:path";
import {{ randomUUID }} from "node:crypto";
import {{ spawn, spawnSync }} from "node:child_process";

const BASE_URL = {encoded_base_url};

function hasCredential() {{
  if (process.env.NAN_MEDIA_API_KEY?.trim()) return true;
  return spawnSync("nanh", ["__media", "credentials"], {{
    stdio: "ignore", env: process.env
  }}).status === 0;
}}

async function runMedia(action, args, output) {{
  await new Promise((resolve, reject) => {{
    const child = spawn("nanh", ["__media", action, "--provider-base-url", BASE_URL, ...args], {{
      stdio: ["ignore", "ignore", "ignore"],
      env: process.env
    }});
    child.once("error", reject);
    child.once("close", code => code === 0 ? resolve() : reject(new Error("NH-MEDIA")));
  }});
  return fs.readFile(output);
}}

async function withTempFile(suffix, fn) {{
  const directory = await fs.mkdtemp(join(tmpdir(), "nanh-media-"));
  const path = join(directory, `${{randomUUID()}}${{suffix}}`);
  try {{ return await fn(path); }} finally {{ await fs.rm(directory, {{ recursive: true, force: true }}); }}
}}

const speech = {{
  id: "nan-harness",
  label: "NaN Kokoro",
  defaultTimeoutMs: 180000,
  isConfigured: hasCredential,
  synthesize: async request => withTempFile(".txt", async input => {{
    const output = `${{input}}.mp3`;
    await fs.writeFile(input, String(request.text ?? request.input ?? ""), "utf8");
    const audioBuffer = await runMedia("tts", ["--input", input, "--output", output, "--voice", String(request.voice ?? request.speakerVoice ?? "af_heart")], output);
    return {{ audioBuffer, outputFormat: "mp3", fileExtension: ".mp3", voiceCompatible: false }};
  }})
}};

const understanding = {{
  id: "nan-harness",
  capabilities: ["audio"],
  resolveAuth: () => ({{ kind: "none", source: "nan-harness" }}),
  transcribeAudio: async request => withTempFile(".audio", async input => {{
    const audio = request.audioBuffer ?? request.buffer ?? request.audio;
    if (!audio) throw new Error("NH-MEDIA-AUDIO");
    await fs.writeFile(input, audio);
    const output = `${{input}}.txt`;
    await runMedia("stt", ["--input", input, "--output", output], output);
    return {{ text: await fs.readFile(output, "utf8") }};
  }})
}};

const image = {{
  id: "nan-harness",
  label: "NaN Flux 2 Klein",
  defaultModel: "flux-2-klein",
  models: ["flux-2-klein"],
  isConfigured: hasCredential,
  capabilities: {{ generate: {{ maxCount: 1 }}, edit: {{ enabled: true, maxCount: 1, maxInputImages: 4 }} }},
  generateImage: async request => withTempFile(".png", async output => {{
    const args = ["--prompt", String(request.prompt ?? ""), "--output", output];
    for (const reference of request.inputImages ?? []) {{
      const path = `${{output}}-${{args.length}}.ref`;
      await fs.writeFile(path, reference.buffer ?? reference);
      args.push("--input-image", path);
    }}
    const buffer = await runMedia("image", args, output);
    return {{ images: [{{ buffer, mimeType: "image/png", fileName: "nan-image.png" }}] }};
  }})
}};

export default definePluginEntry({{
  id: "nan-harness-media",
  name: "nan-media",
  description: "NaN Whisper, Kokoro, and Flux 2 Klein providers",
  register(api) {{
    api.registerSpeechProvider(speech);
    api.registerMediaUnderstandingProvider(understanding);
    api.registerImageGenerationProvider(image);
  }}
}});
"#
    )
}

/// Renders the search plugin used by the persistent `OpenClaw` configuration.
#[must_use]
pub fn render_openclaw_search_plugin() -> String {
    format!(
        r#"{}
import {{ definePluginEntry }} from "openclaw/plugin-sdk/plugin-entry";

const parameters = {{
  type: "object",
  properties: {{
    query: {{ type: "string" }},
    count: {{ type: "integer", minimum: 1, maximum: 20 }}
  }},
  required: ["query"],
  additionalProperties: false
}};

export default definePluginEntry({{
  id: "nan-harness-search",
  name: "nan-search",
  description: "nan-search",
  register(api) {{
    api.registerWebSearchProvider({{
      id: "nan-harness",
      label: "nan-search",
      hint: "nan-search",
      requiresCredential: false,
      envVars: [],
      placeholder: "",
      credentialPath: "",
      getCredentialValue: () => "",
      setCredentialValue: () => {{}},
      createTool: () => ({{
        description: "nan-search",
        parameters,
        execute: async (args, context) => {{
          const query = typeof args.query === "string" ? args.query.trim() : "";
          if (!query) throw new Error("NH-SEARCH-QUERY");
          const count = Number.isInteger(args.count) ? Math.min(Math.max(args.count, 1), 20) : 5;
          const results = await nanSearchResults({{ query, count }}, context?.signal);
          return {{
            query,
            provider: "nan-harness",
            count: results.length,
            externalContent: {{ untrusted: true, source: "web_search", provider: "nan-harness" }},
            results
          }};
        }}
      }})
    }});
  }}
}});
"#,
        saved_search_javascript()
    )
}

#[derive(Debug, Default)]
pub struct OpenClawAdapter;

impl HarnessAdapter for OpenClawAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenClaw
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--model", "--profile", "--dev", "--container"],
        )?;
        let config = openclaw_config(&context.model.resolved_id, context.media)?;
        let mut public_environment = provider_environment();
        public_environment.insert("OPENCLAW_CONFIG_PATH".to_owned(), CONFIG_PATH.to_owned());
        public_environment.insert(
            "OPENCLAW_INCLUDE_ROOTS".to_owned(),
            USER_HOME_PLACEHOLDER.to_owned(),
        );
        let arguments = if context.user_arguments.is_empty() {
            vec!["chat".to_owned()]
        } else {
            context.user_arguments.clone()
        };

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::new(),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "openclaw".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.openclaw"),
                    files: vec![
                        OverlayFile {
                            path: "openclaw.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: "{}".to_owned(),
                            policy: OverlayFilePolicy::Preserve,
                        },
                        OverlayFile {
                            path: "nan-harness.json".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: config,
                            policy: OverlayFilePolicy::Replace,
                        },
                    ]
                    .into_iter()
                    .chain(search_plugin_files())
                    .chain(media_plugin_files(context.media))
                    .collect(),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
