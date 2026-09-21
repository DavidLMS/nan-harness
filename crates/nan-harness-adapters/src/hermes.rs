use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::MediaSelection;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, ConfigurationOverlay,
    HERMES_MODEL_CATALOG_PLACEHOLDER, MEDIA_PROVIDER_BASE_URL_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN,
    NAN_SEARCH_BLOCK_END, OverlayFile, OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER,
    TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    CodingModelProfile, HarnessAdapter, HarnessKind, LaunchPlan, NativeContextLimit, PlanContext,
    PlanError,
};
use std::{collections::BTreeSet, fmt::Write as _};

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "hermes-home";
const CONFIG_PATH: &str = "{artifact:hermes-home}";

fn model_provider_files() -> Vec<OverlayFile> {
    vec![
        OverlayFile {
            path: "plugins/model-providers/nan/__init__.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                r#"from providers import register_provider
from providers.base import ProviderProfile


class NanProviderProfile(ProviderProfile):
    def fetch_models(self, **_kwargs):
        return list(self.fallback_models)


nan = NanProviderProfile(
    name="nan",
    display_name="NaN",
    description="NaN model access",
    env_vars=("NAN_API_KEY",),
    base_url="{PROVIDER_BASE_URL_PLACEHOLDER}",
    auth_type="api_key",
    fallback_models=tuple({HERMES_MODEL_CATALOG_PLACEHOLDER}),
)

register_provider(nan)
"#
            ),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/model-providers/nan/plugin.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "name: nan-provider\nkind: model-provider\nversion: 1.0.0\ndescription: NaN model access\nauthor: NaN\n".to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
    ]
}

/// Files used by both the stable Hermes adapter and the experimental Desktop profile.
#[must_use]
pub fn hermes_search_provider_files() -> Vec<OverlayFile> {
    hermes_search_provider_files_with_context(None, MediaSelection::none())
}

fn hermes_search_provider_files_with_context(
    context_limit: Option<&nan_harness_core::ContextLimit>,
    media: MediaSelection,
) -> Vec<OverlayFile> {
    vec![
        OverlayFile {
            path: "plugins/web/nan_harness/__init__.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "from .provider import NanHarnessWebSearchProvider\n\n\ndef register(ctx):\n    ctx.register_web_search_provider(NanHarnessWebSearchProvider())\n"
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/web/nan_harness/provider.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                r#"import os

import httpx

from agent.web_search_provider import WebSearchProvider


class NanHarnessWebSearchProvider(WebSearchProvider):
    @property
    def name(self):
        return "nan-harness"

    @property
    def display_name(self):
        return "nan-search"

    def is_available(self):
        return bool(os.getenv("NAN_API_KEY", "").strip())

    def search(self, query, limit=5):
        try:
            response = httpx.post(
                "{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search",
                headers={{"Authorization": f"Bearer {{os.environ['NAN_API_KEY']}}"}},
                json={{"query": query, "maxResults": min(max(int(limit), 1), 20)}},
                timeout=60,
            )
            response.raise_for_status()
            results = response.json().get("results", [])
            return {{
                "success": True,
                "data": {{
                    "web": [
                        {{
                            "title": item.get("title", ""),
                            "url": item.get("url", ""),
                            "description": item.get("snippet", ""),
                            "position": position,
                        }}
                        for position, item in enumerate(results, start=1)
                    ]
                }},
            }}
        except Exception:
            return {{"success": False, "error": "NH-SEARCH-HTTP"}}
"#
            ),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/web/nan_harness/plugin.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "name: nan-search\nkind: backend\nversion: 1.0.0\ndescription: nan-search\nauthor: NaN\nprovides_web_providers:\n  - nan-harness\n"
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "config.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: hermes_config_template(context_limit, media),
            policy: OverlayFilePolicy::MergeYaml,
        },
    ]
}

const HERMES_SEARCH_PROVIDER: &str = r#"import atexit
import json
import os
import subprocess
import queue
import sys
import threading
from pathlib import Path
from urllib.parse import urlparse

from agent.web_search_provider import WebSearchProvider


SETUP_GUIDANCE = "NaN web search is not configured; run `nanh search setup`"
_SEARCH_HELPER = None
_SEARCH_REQUEST_ID = 0
_SEARCH_LOCK = threading.Lock()
MAX_RESULTS = 20
MAX_URL_BYTES = 8 * 1024
MAX_TITLE_CHARS = 500
MAX_SNIPPET_CHARS = 2_000


def _config_path():
    override = os.getenv("NAN_HARNESS_CONFIG_DIR")
    if override:
        directory = Path(override)
    elif sys.platform == "darwin":
        directory = Path.home() / "Library" / "Application Support" / "nan-harness"
    elif os.name == "nt":
        directory = Path(os.getenv("APPDATA") or (Path.home() / "AppData" / "Roaming")) / "nan-harness"
    else:
        directory = Path(os.getenv("XDG_CONFIG_HOME") or (Path.home() / ".config")) / "nan-harness"
    return directory / "search.json"


def _search_url():
    try:
        with _config_path().open(encoding="utf-8") as stream:
            config = json.load(stream)
    except FileNotFoundError as error:
        raise RuntimeError(SETUP_GUIDANCE) from error
    except (OSError, TypeError, ValueError) as error:
        raise RuntimeError("NH-SEARCH-CONFIG") from error
    base_url = config.get("baseUrl", "") if isinstance(config, dict) else ""
    if not isinstance(base_url, str):
        raise RuntimeError("NH-SEARCH-CONFIG")
    base_url = base_url.rstrip("/")
    parsed = urlparse(base_url)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.netloc
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
    ):
        raise RuntimeError("NH-SEARCH-CONFIG")
    return f"{base_url}/search"


def _stop_search_helper():
    global _SEARCH_HELPER
    helper = _SEARCH_HELPER
    _SEARCH_HELPER = None
    if helper is None:
        return
    try:
        if helper.poll() is None:
            helper.terminate()
            try:
                helper.wait(timeout=1)
            except subprocess.TimeoutExpired:
                helper.kill()
                helper.wait(timeout=1)
    except OSError:
        pass


atexit.register(_stop_search_helper)


def _helper_exchange(helper, request):
    completed = queue.Queue(maxsize=1)

    def exchange():
        try:
            payload = json.dumps(request) + "\n"
            if len(payload.encode("utf-8")) > 1024 * 1024:
                raise ValueError("request limit")
            helper.stdin.write(payload)
            helper.stdin.flush()
            line = helper.stdout.readline(1024 * 1024 + 1)
            if len(line.encode("utf-8")) > 1024 * 1024:
                raise ValueError("response limit")
            response = json.loads(line)
            if not isinstance(response, dict) or response.get("jsonrpc") != "2.0" or response.get("id") != request["id"]:
                raise ValueError("invalid response")
            completed.put(response)
        except (OSError, ValueError, TypeError):
            completed.put(None)

    threading.Thread(target=exchange, daemon=True).start()
    try:
        response = completed.get(timeout=95)
    except queue.Empty:
        response = None
    if response is None:
        _stop_search_helper()
        raise RuntimeError("NH-SEARCH-HELPER")
    return response


def _search_helper_request(method, params):
    global _SEARCH_HELPER, _SEARCH_REQUEST_ID
    if not _SEARCH_LOCK.acquire(timeout=95):
        raise RuntimeError("NH-SEARCH-HELPER")
    try:
        if _SEARCH_HELPER is None or _SEARCH_HELPER.poll() is not None:
            executable = os.getenv("NAN_HARNESS_BIN", "nanh")
            _SEARCH_HELPER = subprocess.Popen(
                [executable, "__search-mcp"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                encoding="utf-8",
                bufsize=1,
            )
        _SEARCH_REQUEST_ID += 1
        response = _helper_exchange(_SEARCH_HELPER, {
            "jsonrpc": "2.0",
            "id": _SEARCH_REQUEST_ID,
            "method": method,
            "params": params,
        })
        if "error" in response or response.get("result", {}).get("isError"):
            content = response.get("result", {}).get("content", [])
            message = next((item.get("text") for item in content if item.get("type") == "text"), None)
            raise RuntimeError(message or "NH-SEARCH-HELPER")
        return response.get("result", {})
    finally:
        _SEARCH_LOCK.release()


def _search_with_helper(query, limit):
    result = _search_helper_request(
        "tools/call",
        {
            "name": "web_search",
            "arguments": {"query": query, "max_results": limit},
        },
    )
    return result.get("structuredContent", {}).get("results", [])


class NanHarnessWebSearchProvider(WebSearchProvider):
    @property
    def name(self):
        return "nan-harness"

    @property
    def display_name(self):
        return "nan-search"

    def is_available(self):
        return True

    def search(self, query, limit=5):
        if not isinstance(query, str) or not query.strip() or len(query.encode("utf-8")) > 8 * 1024:
            return {"success": False, "error": "NH-SEARCH-QUERY"}
        try:
            _search_url()
        except RuntimeError as error:
            return {"success": False, "error": str(error)}
        requested = limit if isinstance(limit, int) and not isinstance(limit, bool) else 5
        max_results = min(max(requested, 1), MAX_RESULTS)
        try:
            results = _search_with_helper(query.strip(), max_results)
            web_results = _normalise_results(results, max_results)
            return {"success": True, "data": {"web": web_results}}
        except Exception:
            return {"success": False, "error": "NH-SEARCH-HTTP"}


def _normalise_results(results, max_results):
    if not isinstance(results, list):
        return []
    web_results = []
    for item in results:
        if not isinstance(item, dict):
            continue
        title = item.get("title")
        url = item.get("url")
        if not isinstance(title, str) or not isinstance(url, str):
            continue
        title = title.strip()
        url = url.strip()
        if not title or not url or not _safe_result_url(url):
            continue
        snippet = item.get("content", item.get("snippet", ""))
        if not isinstance(snippet, str):
            snippet = ""
        web_results.append(
            {
                "title": title[:MAX_TITLE_CHARS],
                "url": url,
                "description": snippet[:MAX_SNIPPET_CHARS],
                "position": len(web_results) + 1,
            }
        )
        if len(web_results) == max_results:
            break
    return web_results


def _safe_result_url(url):
    if len(url.encode("utf-8")) > MAX_URL_BYTES or any(char.isspace() for char in url):
        return False
    try:
        parsed = urlparse(url)
        hostname = parsed.hostname
        parsed.port
    except ValueError:
        return False
    return (
        parsed.scheme.lower() in {"http", "https"}
        and bool(parsed.netloc)
        and bool(hostname)
        and parsed.username is None
        and parsed.password is None
    )
"#;

/// Renders the provider used by the persistent Hermes configuration.
#[must_use]
pub fn render_hermes_search_provider() -> String {
    HERMES_SEARCH_PROVIDER.to_owned()
}

fn hermes_config_template(
    context_limit: Option<&nan_harness_core::ContextLimit>,
    media: MediaSelection,
) -> String {
    let compression = context_limit
        .and_then(|limit| match limit.native {
            NativeContextLimit::HermesThreshold { threshold_tokens } => Some(format!(
                "\"compression\": {{\"enabled\": true, \"threshold_tokens\": {threshold_tokens}}},"
            )),
            _ => None,
        })
        .unwrap_or_default();
    let mut media_fields = String::new();
    if media.stt {
        let _ = write!(
            media_fields,
            "\"stt\":{{\"provider\":\"nan-whisper\",\"providers\":{{\"nan-whisper\":{}}}}},",
            hermes_command_provider("stt", "whisper-1", base_url_placeholder())
        );
    }
    if media.tts {
        let _ = write!(
            media_fields,
            "\"tts\":{{\"provider\":\"nan-kokoro\",\"providers\":{{\"nan-kokoro\":{}}}}},",
            hermes_command_provider("tts", "kokoro", base_url_placeholder())
        );
    }
    if media.image {
        media_fields.push_str("\"image_gen\":{\"provider\":\"nan-harness\"},");
    }
    let plugin_field = if media.image {
        format!(
            "\"plugins\":{{\"enabled\":[\"image_gen/nan_harness\"{NAN_SEARCH_BLOCK_BEGIN},\"web/nan_harness\"{NAN_SEARCH_BLOCK_END}]}}"
        )
    } else {
        format!(
            "\"plugins\":{{\"enabled\":[{NAN_SEARCH_BLOCK_BEGIN}\"web/nan_harness\"{NAN_SEARCH_BLOCK_END}]}}"
        )
    };
    format!(
        "{{{compression}{media_fields}{plugin_field}{NAN_SEARCH_BLOCK_BEGIN},\"web\":{{\"search_backend\":\"nan-harness\"}}{NAN_SEARCH_BLOCK_END}}}\n"
    )
}

fn base_url_placeholder() -> &'static str {
    MEDIA_PROVIDER_BASE_URL_PLACEHOLDER
}

fn hermes_command_provider(kind: &str, model: &str, base_url: &str) -> String {
    serde_json::to_string(&hermes_command_provider_config(kind, model, base_url))
        .unwrap_or_else(|_| "{}".to_owned())
}

/// Returns the native Hermes command-provider configuration for one media action.
#[must_use]
pub fn hermes_command_provider_config(
    kind: &str,
    model: &str,
    base_url: &str,
) -> serde_json::Value {
    hermes_command_provider_config_for_platform(kind, model, base_url, cfg!(windows))
}

fn hermes_command_provider_config_for_platform(
    kind: &str,
    model: &str,
    base_url: &str,
    windows: bool,
) -> serde_json::Value {
    let options = if kind == "tts" {
        format!(
            " --voice {} --format {}",
            shell_quote_for_platform("{voice}", windows),
            shell_quote_for_platform("{format}", windows)
        )
    } else {
        String::new()
    };
    let command = format!(
        "nanh __media {kind} --provider-base-url {} --input {} --output {}{options}",
        shell_quote_for_platform(base_url, windows),
        shell_quote_for_platform("{input_path}", windows),
        shell_quote_for_platform("{output_path}", windows),
    );
    let mut config = serde_json::json!({
        "type": "command",
        "command": command,
        "model": model,
        "env_passthrough": ["NAN_MEDIA_API_KEY", "NAN_API_KEY"]
    });
    if kind == "tts" {
        config["voice"] = serde_json::json!("af_heart");
        config["format"] = serde_json::json!("mp3");
    }
    config
}

fn shell_quote_for_platform(value: &str, windows: bool) -> String {
    if windows {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::hermes_command_provider_config_for_platform;

    #[test]
    fn windows_command_provider_quotes_paths_for_cmd() {
        let provider = hermes_command_provider_config_for_platform(
            "tts",
            "kokoro",
            "https://api.nan.test/v1",
            true,
        );
        let command = provider["command"]
            .as_str()
            .expect("command should be a string");
        assert_eq!(
            command,
            "nanh __media tts --provider-base-url \"https://api.nan.test/v1\" --input \"{input_path}\" --output \"{output_path}\" --voice \"{voice}\" --format \"{format}\""
        );
    }

    #[test]
    fn posix_command_provider_keeps_shell_safe_single_quotes() {
        let provider = hermes_command_provider_config_for_platform(
            "stt",
            "whisper-1",
            "https://api.nan.test/v1",
            false,
        );
        let command = provider["command"]
            .as_str()
            .expect("command should be a string");
        assert_eq!(
            command,
            "nanh __media stt --provider-base-url 'https://api.nan.test/v1' --input '{input_path}' --output '{output_path}'"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_provider_preserves_arguments_through_cmd() {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        let directory =
            std::env::temp_dir().join(format!("nanh hermes command {}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temporary command directory should exist");
        let script = directory.join("capture.cmd");
        let script_body = "@echo off\r\n:next\r\nif \"%~1\"==\"\" exit /b 0\r\necho(%~1\r\nshift\r\ngoto next\r\n";
        std::fs::write(&script, script_body).expect("capture script should write");

        let provider = hermes_command_provider_config_for_platform(
            "tts",
            "kokoro",
            "https://api.nan.test/v1",
            true,
        );
        let command = provider["command"]
            .as_str()
            .expect("command should be a string")
            .replacen("nanh", "capture.cmd", 1)
            .replace("{voice}", "af voice")
            .replace("{format}", "mp3")
            .replace("{input_path}", r"C:\Audio Files\input.wav")
            .replace("{output_path}", r"C:\Audio Files\output.mp3");
        let output = Command::new("cmd.exe")
            .current_dir(&directory)
            .args(["/d", "/s", "/c"])
            .raw_arg(format!("\"{command}\""))
            .output()
            .expect("cmd should start");
        std::fs::remove_dir_all(&directory).expect("capture directory should be removed");
        let actual = String::from_utf8(output.stdout).expect("arguments should be UTF-8");
        let actual = actual.lines().collect::<Vec<_>>();
        assert!(
            output.status.success(),
            "cmd should execute the provider command"
        );
        assert_eq!(
            actual,
            [
                "__media",
                "tts",
                "--provider-base-url",
                "https://api.nan.test/v1",
                "--input",
                r"C:\Audio Files\input.wav",
                "--output",
                r"C:\Audio Files\output.mp3",
                "--voice",
                "af voice",
                "--format",
                "mp3",
            ]
        );
    }
}

/// Renders the Hermes image generation plugin for a persistent or launch-scoped home.
#[must_use]
pub fn render_hermes_image_plugin(base_url: &str) -> String {
    format!(
        r#"import os
import subprocess
import tempfile
import uuid
from pathlib import Path

from agent.image_gen_provider import ImageGenProvider, success_response


class NanHarnessImageProvider(ImageGenProvider):
    name = "nan-harness"
    display_name = "NaN Flux 2 Klein"
    def capabilities(self):
        return {{"modalities": ["text", "image"], "max_reference_images": 4}}

    def is_available(self):
        if os.getenv("NAN_MEDIA_API_KEY", "").strip():
            return True
        try:
            return subprocess.run(
                ["nanh", "__media", "credentials"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            ).returncode == 0
        except OSError:
            return False

    def list_models(self):
        return [{{"id": "flux-2-klein", "name": "Flux 2 Klein"}}]

    def generate(self, prompt, aspect_ratio="landscape", *, image_url=None,
                 reference_image_urls=None, model="flux-2-klein", **_kwargs):
        references = ([image_url] if image_url else []) + list(reference_image_urls or [])
        with tempfile.TemporaryDirectory(prefix="nanh-image-") as directory:
            output = Path(directory) / "image.png"
            command = [
                "nanh", "__media", "image", "--provider-base-url", {base_url:?},
                "--model", str(model), "--prompt", str(prompt), "--output", str(output)
            ]
            for index, image in enumerate(references):
                reference = Path(image)
                if isinstance(image, str) and image.startswith(("http://", "https://")):
                    import urllib.request
                    reference = Path(directory) / f"reference-{{index}}.bin"
                    with urllib.request.urlopen(image, timeout=60) as response:
                        data = response.read(25 * 1024 * 1024 + 1)
                        if len(data) > 25 * 1024 * 1024:
                            return {{"success": False, "error": "NH-MEDIA-REFERENCE"}}
                        reference.write_bytes(data)
                if reference.exists():
                    command.extend(["--input-image", str(reference)])
            completed = subprocess.run(
                command, env=os.environ.copy(),
                capture_output=True, text=True, check=False
            )
            if completed.returncode != 0 or not output.is_file():
                return {{"success": False, "error": "NH-MEDIA-IMAGE"}}
            hermes_home = os.getenv("HERMES_HOME")
            image_directory = Path(hermes_home) if hermes_home else Path.home() / ".hermes"
            image_directory = image_directory / "cache" / "images"
            image_directory.mkdir(parents=True, exist_ok=True)
            image_path = image_directory / f"nan-image-{{uuid.uuid4().hex}}.png"
            image_path.write_bytes(output.read_bytes())
            return success_response(
                image=str(image_path), model=str(model), prompt=str(prompt),
                aspect_ratio=str(aspect_ratio), provider=self.name,
                modality="image" if references else "text",
            )


def register(ctx):
    ctx.register_image_gen_provider(NanHarnessImageProvider())
"#
    )
}

/// Render the provider entry shared by the experimental persistent Desktop profile.
#[must_use]
pub fn render_hermes_desktop_provider_block(
    base_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
) -> String {
    let yaml_string =
        |value: &str| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned());
    let mut block = format!(
        "  nan:\n    name: NaN\n    base_url: {}\n    key_env: NAN_API_KEY\n    transport: openai_chat\n    model: {}\n    default_model: {}\n    discover_models: true\n    models:",
        yaml_string(base_url),
        yaml_string(selected_model),
        yaml_string(selected_model),
    );
    for model in models {
        use std::fmt::Write as _;
        let _ = write!(
            block,
            "\n      {}:\n        context_length: {}",
            yaml_string(&model.id),
            model.context_window
        );
    }
    block
}

#[derive(Debug, Default)]
pub struct HermesAdapter;

impl HarnessAdapter for HermesAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Hermes
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m", "--provider"])?;
        let mut public_environment = provider_environment();
        public_environment.insert("HERMES_HOME".to_owned(), CONFIG_PATH.to_owned());
        let mut arguments = vec![
            "--provider".to_owned(),
            "nan".to_owned(),
            "--model".to_owned(),
            context.model.resolved_id.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "CUSTOM_BASE_URL".to_owned(),
                    "HERMES_INFERENCE_MODEL".to_owned(),
                    "HERMES_INFERENCE_PROVIDER".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "hermes".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.hermes"),
                    files: model_provider_files()
                        .into_iter()
                        .chain(hermes_search_provider_files_with_context(
                            context.context_limit.as_ref(),
                            context.media,
                        ))
                        .chain(hermes_media_overlay_files(context.media))
                        .collect(),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}

fn hermes_media_overlay_files(media: MediaSelection) -> Vec<OverlayFile> {
    if !media.image {
        return Vec::new();
    }
    vec![
        OverlayFile {
            path: "plugins/image_gen/nan_harness/__init__.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "from .provider import NanHarnessImageProvider\n\n\ndef register(ctx):\n    ctx.register_image_gen_provider(NanHarnessImageProvider())\n".to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/image_gen/nan_harness/provider.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: render_hermes_image_plugin(MEDIA_PROVIDER_BASE_URL_PLACEHOLDER),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/image_gen/nan_harness/plugin.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "name: nan-image\nkind: backend\nversion: 1.0.0\ndescription: NaN image generation\nauthor: NaN\nprovides_image_gen_providers:\n  - nan-harness\n".to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
    ]
}
