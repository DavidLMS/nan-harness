use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, ConfigurationOverlay,
    HERMES_MODEL_CATALOG_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    CodingModelProfile, HarnessAdapter, HarnessKind, LaunchPlan, NativeContextLimit, PlanContext,
    PlanError,
};
use std::collections::BTreeSet;

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
    hermes_search_provider_files_with_context(None)
}

fn hermes_search_provider_files_with_context(
    context_limit: Option<&nan_harness_core::ContextLimit>,
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
            content_template: hermes_config_template(context_limit),
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

fn hermes_config_template(context_limit: Option<&nan_harness_core::ContextLimit>) -> String {
    let compression = context_limit
        .and_then(|limit| match limit.native {
            NativeContextLimit::HermesThreshold { threshold_tokens } => Some(format!(
                "\"compression\": {{\"enabled\": true, \"threshold_tokens\": {threshold_tokens}}},"
            )),
            _ => None,
        })
        .unwrap_or_default();
    format!(
        "{{{compression}{NAN_SEARCH_BLOCK_BEGIN}\"plugins\": {{\"enabled\": [\"web/nan_harness\"]}}, \"web\": {{\"search_backend\": \"nan-harness\"}}{NAN_SEARCH_BLOCK_END}}}\n"
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
                        ))
                        .collect(),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
