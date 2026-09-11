use super::*;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[test]
fn persistent_search_plugins_have_valid_source_syntax() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut node = match Command::new("node")
        .args(["--input-type=module", "--check"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Node syntax check should start: {error}"),
    };
    node.stdin
        .take()
        .expect("Node stdin should be available")
        .write_all(openclaw_search_plugin().as_bytes())
        .expect("plugin source should write");
    let output = node
        .wait_with_output()
        .expect("Node syntax check should finish");
    assert!(
        output.status.success(),
        "OpenClaw plugin syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for mode in [PiSearchMode::Auto, PiSearchMode::Force] {
        let mut node = Command::new("node")
            .args(["--input-type=module", "--check"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Node syntax check should start after the first successful invocation");
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(render_pi_search_extension("https://api.nan.test/v1", mode).as_bytes())
            .expect("Pi extension source should write");
        let output = node
            .wait_with_output()
            .expect("Node syntax check should finish");
        assert!(
            output.status.success(),
            "Pi extension syntax failed in {mode:?} mode: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut python = match Command::new("python3")
        .args([
            "-c",
            "import sys; compile(sys.stdin.read(), 'provider.py', 'exec')",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Python syntax check should start: {error}"),
    };
    python
        .stdin
        .take()
        .expect("Python stdin should be available")
        .write_all(hermes_search_provider().as_bytes())
        .expect("provider source should write");
    let output = python
        .wait_with_output()
        .expect("Python syntax check should finish");
    assert!(
        output.status.success(),
        "Hermes provider syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn persistent_search_plugins_use_saved_configuration_without_credentials() {
    let sources = [
        (
            "Pi",
            render_pi_search_extension("https://api.nan.test/v1", PiSearchMode::Auto),
        ),
        (
            "OMP",
            render_omp_search_extension("https://api.nan.test/v1", OmpSearchMode::Auto),
        ),
        ("Hermes", hermes_search_provider()),
        ("OpenClaw", openclaw_search_plugin()),
    ];
    for (name, source) in sources {
        assert!(source.contains("NAN_HARNESS_CONFIG_DIR"), "{name}");
        assert!(source.contains("search.json"), "{name}");
        assert!(
            source.contains("NaN web search is not configured; run `nanh search setup`"),
            "{name}"
        );
        assert!(
            !source.contains("api.nan.test"),
            "{name} retained provider endpoint"
        );
        assert!(
            !source.contains("NAN_API_KEY"),
            "{name} retained provider credential"
        );
        assert!(
            !source.contains("authorization"),
            "{name} retained auth header"
        );
        assert!(
            !source.contains("Bearer"),
            "{name} retained bearer credential"
        );
    }
}

#[test]
fn pi_search_extension_runtime_detection_respects_auto_and_force() {
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    for (mode, existing_search, expected_registrations) in [
        (PiSearchMode::Auto, true, 0),
        (PiSearchMode::Auto, false, 1),
        (PiSearchMode::Force, true, 1),
    ] {
        let mut source = render_pi_search_extension("https://api.nan.test/v1", mode)
            .replacen(
                "import { Type } from \"@earendil-works/pi-ai\";",
                "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
                1,
            )
            .replacen(
                "export default function registerNanSearch",
                "function registerNanSearch",
                1,
            );
        let inventory = if existing_search {
            "[{ name: \"web_search\" }]"
        } else {
            "[]"
        };
        write!(
            source,
            r#"
let discover;
const registrations = [];
const pi = {{
  on(event, handler) {{
    if (event !== "resources_discover") throw new Error(`unexpected event: ${{event}}`);
    discover = handler;
  }},
  getAllTools() {{ return {inventory}; }},
  registerTool(tool) {{ registrations.push(tool); }}
}};
registerNanSearch(pi);
discover();
if (registrations.length !== {expected_registrations}) {{
  throw new Error(`expected {expected_registrations} registrations, got ${{registrations.length}}`);
}}
"#
        )
        .expect("runtime check source should render");

        let mut node = match Command::new("node")
            .args(["--input-type=module"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("Node runtime check should start: {error}"),
        };
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(source.as_bytes())
            .expect("runtime check source should write");
        let output = node
            .wait_with_output()
            .expect("Node runtime check should finish");
        assert!(
            output.status.success(),
            "Pi runtime detection failed for {mode:?} with existing_search={existing_search}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(unix)]
#[test]
fn native_search_plugins_execute_through_a_lifecycle_helper_and_release_it() {
    if Command::new("node").arg("--version").output().is_err()
        || Command::new("python3").arg("--version").output().is_err()
    {
        return;
    }

    let directory = tempdir().expect("isolated native search fixture");
    let config = directory.path().join("config");
    let marker = directory.path().join("lifecycle");
    fs::create_dir_all(&config).expect("config directory");
    fs::create_dir_all(&marker).expect("lifecycle directory");
    fs::write(
        config.join("search.json"),
        r#"{"baseUrl":"http://127.0.0.1:9911"}"#,
    )
    .expect("saved local search configuration");
    let helper = directory.path().join("synthetic-search-helper.py");
    fs::write(&helper, synthetic_search_helper()).expect("synthetic helper");
    fs::set_permissions(&helper, Permissions::from_mode(0o700))
        .expect("helper should be executable");

    run_node_native_plugins(&config, &helper, &marker);
    run_hermes_native(&config, &helper, &marker);
}

#[cfg(unix)]
fn run_node_native_plugins(config: &Path, helper: &Path, marker: &Path) {
    let node_sources = [
        (
            "pi",
            render_pi_search_extension("https://api.nan.test/v1", PiSearchMode::Force)
                .replacen(
                    "import { Type } from \"@earendil-works/pi-ai\";",
                    "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
                    1,
                )
                .replacen(
                    "export default function registerNanSearch",
                    "function registerNanSearch",
                    1,
                ),
            r#"
let registered;
const pi = {
  on(_event, handler) { handler(); },
  getAllTools() { return []; },
  registerTool(tool) { registered = tool; }
};
registerNanSearch(pi);
const result = await registered.execute("test", { query: "cold local", maxResults: 2 }, undefined, undefined, {});
if (result.details.results[0].title !== "Synthetic result") throw new Error("Pi did not search");
globalThis.nanTestTimeoutMs = 250;
let timedOut = false;
try { await registered.execute("timeout", { query: "stall" }, undefined, undefined, {}); }
catch (error) { timedOut = error.message === "NH-SEARCH-HELPER"; }
delete globalThis.nanTestTimeoutMs;
if (!timedOut) throw new Error("unresponsive helper did not time out");
const retried = await registered.execute("retry", { query: "retry" }, undefined, undefined, {});
if (retried.details.results[0].title !== "Synthetic result") throw new Error("helper did not restart");

"#,
        ),
        (
            "omp",
            render_omp_search_extension("https://api.nan.test/v1", OmpSearchMode::Force)
                .replacen(
                    "import { Type } from \"@oh-my-pi/pi-ai\";",
                    "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
                    1,
                )
                .replacen(
                    "import { settings } from \"@oh-my-pi/pi-coding-agent\";",
                    "const settings = { get: () => [] };",
                    1,
                )
                .replacen(
                    "import { getSearchProvider, setExcludedSearchProviders } from \"@oh-my-pi/pi-coding-agent/web/search\";",
                    "const getSearchProvider = async () => ({ isAvailable: async () => false }); const setExcludedSearchProviders = () => {};",
                    1,
                )
                .replacen(
                    "export default function registerNanSearch",
                    "function registerNanSearch",
                    1,
                ),
            r#"
let registered;
const pi = { registerTool(tool) { registered = tool; } };
registerNanSearch(pi);
const result = await registered.execute("test", { query: "cold local", limit: 2 }, undefined, undefined, {});
if (result.details.results[0].title !== "Synthetic result") throw new Error("OMP did not search");

"#,
        ),
        (
            "openclaw",
            render_openclaw_search_plugin()
                .replacen(
                    "import { definePluginEntry } from \"openclaw/plugin-sdk/plugin-entry\";",
                    "const definePluginEntry = (entry) => entry;",
                    1,
                )
                .replacen("export default definePluginEntry", "const plugin = definePluginEntry", 1),
            r#"
let provider;
plugin.register({ registerWebSearchProvider(value) { provider = value; } });
const result = await provider.createTool().execute({ query: "cold local", count: 2 }, {});
if (result.results[0].title !== "Synthetic result") throw new Error("OpenClaw did not search");

"#,
        ),
    ];

    for (name, source, invocation) in node_sources {
        let mut script = source.replace(
            "setTimeout(fail, NAN_SEARCH_HELPER_TIMEOUT_MS)",
            "setTimeout(fail, globalThis.nanTestTimeoutMs ?? NAN_SEARCH_HELPER_TIMEOUT_MS)",
        );
        script.push_str(invocation);
        run_native_child(
            "node",
            &["--input-type=module"],
            &script,
            config,
            helper,
            marker,
            name,
        );
    }
}

#[cfg(unix)]
fn run_hermes_native(config: &Path, helper: &Path, marker: &Path) {
    let mut hermes = hermes_search_provider().replace(
        "timeout=95",
        "timeout=globals().get('_nan_test_timeout', 95)",
    );
    hermes.push_str(
        r#"
provider = NanHarnessWebSearchProvider()
result = provider.search("cold local", 2)
if result["data"]["web"][0]["title"] != "Synthetic result":
    raise RuntimeError("Hermes did not search")
_nan_test_timeout = 0.25
if provider.search("stall")["success"]:
    raise RuntimeError("unresponsive Hermes helper did not time out")
del _nan_test_timeout
if not provider.search("retry")["success"]:
    raise RuntimeError("Hermes helper did not restart")
"#,
    );
    let prefix = r#"
import sys, types
agent = types.ModuleType("agent")
agent.__path__ = []
provider_module = types.ModuleType("agent.web_search_provider")
class WebSearchProvider:
    pass
provider_module.WebSearchProvider = WebSearchProvider
sys.modules["agent"] = agent
sys.modules["agent.web_search_provider"] = provider_module
"#;
    let mut hermes_source = prefix.to_owned();
    hermes_source.push_str(&hermes);
    run_native_child(
        "python3",
        &["-"],
        &hermes_source,
        config,
        helper,
        marker,
        "hermes",
    );
}

#[cfg(unix)]
fn run_native_child(
    executable: &str,
    arguments: &[&str],
    source: &str,
    config: &Path,
    helper: &Path,
    marker: &Path,
    name: &str,
) {
    let mut child = Command::new(executable)
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", config)
        .env("NAN_HARNESS_BIN", helper)
        .env_remove("NAN_API_KEY")
        .env("NAN_SYNTHETIC_MARKER", marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{name} native child should start: {error}"));
    child
        .stdin
        .take()
        .expect("native child stdin")
        .write_all(source.as_bytes())
        .expect("native child source");
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().expect("inspect native child").is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} kept the native session alive after its work finished");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("{name} native child should finish: {error}"));
    assert!(
        output.status.success(),
        "{name} native child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let started: Vec<_> = fs::read_dir(marker)
        .expect("helper markers")
        .map(|entry| {
            entry
                .expect("helper marker")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter_map(|name| name.strip_prefix("started-").map(str::to_owned))
        .collect();
    assert!(!started.is_empty(), "{name} did not start helper");
    assert!(
        started
            .iter()
            .any(|id| marker.join(format!("interest-{id}")).exists()),
        "{name} did not call helper"
    );
    let all_released = || {
        started
            .iter()
            .all(|id| marker.join(format!("released-{id}")).exists())
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !all_released() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        all_released(),
        "{name} did not release every helper generation"
    );
    for entry in fs::read_dir(marker).expect("lifecycle directory") {
        let path = entry.expect("lifecycle marker").path();
        fs::remove_file(path).expect("reset lifecycle marker");
    }
}

#[cfg(unix)]
fn synthetic_search_helper() -> &'static str {
    r#"#!/usr/bin/env python3
import json
import os
import signal
import sys
import time
from pathlib import Path

marker = Path(os.environ["NAN_SYNTHETIC_MARKER"])
(marker / f"started-{os.getpid()}").touch()

def release(_signum, _frame):
    (marker / f"released-{os.getpid()}").touch()
    raise SystemExit(0)

signal.signal(signal.SIGTERM, release)
signal.signal(signal.SIGINT, release)
for line in sys.stdin:
    request = json.loads(line)
    if request.get("params", {}).get("arguments", {}).get("query") == "stall":
        time.sleep(5)
    if request["method"] == "initialize":
        result = {"protocolVersion": "2025-06-18"}
    else:
        (marker / f"interest-{os.getpid()}").touch()
        result = {"structuredContent": {"results": [{"title": "Synthetic result", "url": "https://example.test/result", "snippet": "synthetic"}]}}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
(marker / f"released-{os.getpid()}").touch()
"#
}
