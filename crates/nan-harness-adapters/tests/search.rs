use nan_harness_adapters::{
    PiSearchMode, render_hermes_search_provider, render_pi_search_extension,
};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

const PI_BEHAVIOR_SCRIPT: &str = r#"
let discover;
let searchTool;
const pi = {
  on(event, handler) {
    if (event !== "resources_discover") throw new Error(`unexpected event: ${event}`);
    discover = handler;
  },
  getAllTools() { return []; },
  registerTool(tool) { searchTool = tool; }
};
registerNanSearch(pi);
discover();
if (!searchTool) throw new Error("search tool was not registered");

let helperCalls = 0;
nanSearchMcp = async (params) => {
  helperCalls += 1;
  if (params.query === "bounded") {
    if (params.maxResults !== 1) throw new Error("limit hint changed");
    return [
          { title: "blocked", url: "https://blocked.test/a", content: "blocked" },
          { title: " " + "🙂".repeat(600), url: "https://allowed.test/b", content: "x".repeat(2100) },
          { title: "second", url: "https://allowed.test/second", content: "second" }
    ];
  }
  if (params.maxResults !== 20) throw new Error("unbounded limit hint changed");
  return [
        { title: "exact", url: "https://allowed.test/base" },
        { title: "subdomain", url: "https://sub.allowed.test/base/child" },
        { title: "path boundary", url: "https://allowed.test/baseball" },
        { title: "blocked precedence", url: "https://allowed.test/base/private" },
        { title: "outside path", url: "https://allowed.test/other" },
        { title: "host boundary", url: "https://notallowed.test/base" },
        { title: "missing URL" },
        { title: "unsafe", url: "javascript:alert(1)" },
        { title: "oversized", url: "https://allowed.test/" + "x".repeat(9000) }
  ];
};

const bounded = await searchTool.execute("call-1", {
  query: " bounded ",
  maxResults: 1,
  allowedDomains: ["allowed.test"],
  blockedDomains: ["blocked.test"]
}, undefined, undefined, undefined);
if (bounded.details.results.length !== 1) throw new Error("server result count was trusted");
if (bounded.details.results[0].url !== "https://allowed.test/b") throw new Error("domain filters failed");
if (Array.from(bounded.details.results[0].title).length !== 500) throw new Error("title bound was not enforced");
if (bounded.details.results[0].snippet.length !== 2000) throw new Error("snippet bound was not enforced");

const paths = await searchTool.execute("call-2", {
  query: "paths",
  maxResults: 999,
  allowedDomains: ["allowed.test/base"],
  blockedDomains: ["allowed.test/base/private/"]
}, undefined, undefined, undefined);
const pathUrls = paths.details.results.map((result) => result.url);
if (JSON.stringify(pathUrls) !== JSON.stringify([
  "https://allowed.test/base",
  "https://sub.allowed.test/base/child"
])) throw new Error(`path filtering failed: ${JSON.stringify(pathUrls)}`);
if (helperCalls !== 2) throw new Error("unexpected fetch count");

let malformedFilterRejected = false;
try {
  await searchTool.execute("call-3", {
    query: "paths",
    allowedDomains: ["allowed.test", "not a domain"]
  }, undefined, undefined, undefined);
} catch (error) {
  malformedFilterRejected = error instanceof Error && error.message === "NH-SEARCH-DOMAIN";
}
if (!malformedFilterRejected) throw new Error("malformed domain filter was accepted");

let emptyQueryRejected = false;
try {
  await searchTool.execute("call-4", { query: "   " }, undefined, undefined, undefined);
} catch (error) {
  emptyQueryRejected = error instanceof Error && error.message === "NH-SEARCH-QUERY";
}
if (!emptyQueryRejected) throw new Error("empty query was accepted");
"#;

struct SearchFixture {
    directory: PathBuf,
}

impl SearchFixture {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "nan-harness-search-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("isolated search fixture directory should be created");
        fs::write(
            directory.join("search.json"),
            br#"{"baseUrl":"https://search.nan.test"}"#,
        )
        .expect("synthetic search configuration should be written");
        Self { directory }
    }
}

impl Drop for SearchFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn pi_search_filters_domains_and_results_before_returning_them() {
    let fixture = SearchFixture::new();
    let source = render_pi_search_extension("https://unused.nan.test/v1", PiSearchMode::Force)
        .replace(
            "import { Type } from \"@earendil-works/pi-ai\";",
            "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
        )
        .replace(
            "export default function registerNanSearch",
            "function registerNanSearch",
        )
        + PI_BEHAVIOR_SCRIPT;

    let Some(output) = run_optional_child("node", &["--input-type=module"], &source, &fixture)
    else {
        return;
    };
    assert!(
        output.status.success(),
        "Pi search behavior failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn hermes_search_skips_unsafe_results_and_enforces_bounds() {
    let fixture = SearchFixture::new();
    let wrapper = r#"
import sys
import types

agent = types.ModuleType("agent")
provider_module = types.ModuleType("agent.web_search_provider")
class WebSearchProvider:
    pass
provider_module.WebSearchProvider = WebSearchProvider
sys.modules["agent"] = agent
sys.modules["agent.web_search_provider"] = provider_module

exec(sys.stdin.read(), globals())
def fake_search(query, limit):
    assert query == "question"
    assert limit == 2
    return [
            {},
            {"title": "unsafe", "url": "javascript:alert(1)"},
            {"title": "credential", "url": "https://user:secret@allowed.test/a"},
            {"title": " " + "🙂" * 600, "url": "https://allowed.test/a", "content": "x" * 2100},
            {"title": "second", "url": "https://sub.allowed.test/b", "snippet": "second"},
            {"title": "oversized", "url": "https://allowed.test/" + "x" * 9000}
        ]
_search_with_helper = fake_search
provider = NanHarnessWebSearchProvider()
result = provider.search(" question ", limit=2)
assert result["success"] is True
web = result["data"]["web"]
assert [item["url"] for item in web] == [
    "https://allowed.test/a",
    "https://sub.allowed.test/b",
]
assert web[0]["position"] == 1
assert web[1]["position"] == 2
assert len(web[0]["title"]) == 500
assert len(web[0]["description"]) == 2000
assert provider.search("   ")["error"] == "NH-SEARCH-QUERY"
"#;
    let Some(output) = run_optional_child(
        "python3",
        &["-c", wrapper],
        &render_hermes_search_provider(),
        &fixture,
    ) else {
        return;
    };
    assert!(
        output.status.success(),
        "Hermes search behavior failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_optional_child(
    command: &str,
    arguments: &[&str],
    source: &str,
    fixture: &SearchFixture,
) -> Option<std::process::Output> {
    let mut child = match Command::new(command)
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", &fixture.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("{command} behavior check should start: {error}"),
    };
    child
        .stdin
        .take()
        .expect("behavior check stdin should be available")
        .write_all(source.as_bytes())
        .expect("behavior check source should write");
    Some(
        child
            .wait_with_output()
            .expect("behavior check should finish"),
    )
}
