use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use nan_harness_test_support::{conformance::assert_success, terminal::TerminalCommand};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};

pub const MARKER: &str = "NAN_REASONING_OK";
const MODELS: [&str; 5] = [
    "qwen3.6",
    "gemma4",
    "glm5.3-flash",
    "deepseek-v4-flash",
    "mimo-v2.6-flash",
];

pub struct Fixture {
    root: tempfile::TempDir,
    pub home: PathBuf,
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    server: JoinHandle<()>,
}

impl Fixture {
    pub async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let state = home.join(".nan-harness");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("nan-api-key"), "synthetic-native-reasoning-key").unwrap();
        std::fs::write(
            state.join("credential.json"),
            r#"{"schemaVersion":1,"backend":"private-file"}"#,
        )
        .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/v1/models",
                get(|| async {
                    Json(json!({"data":MODELS.map(|id|json!({"id":id,"object":"model"}))}))
                }),
            )
            .route("/v1/chat/completions", post(chat))
            .with_state(Arc::clone(&requests));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            root,
            home,
            endpoint,
            requests,
            server,
        }
    }

    pub fn command(&self, program: impl Into<PathBuf>) -> TerminalCommand {
        TerminalCommand::new(program, self.root.path())
            .clear_environment()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("XDG_CACHE_HOME", self.home.join("cache"))
            .env("NAN_HARNESS_CONFIG_DIR", self.home.join(".nan-harness"))
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env("QWEN_HOME", self.home.join(".qwen"))
            .env("CODEX_HOME", self.home.join(".codex"))
            .env("ZCODE_DATA_BASE_DIR", self.home.join("zcode"))
            .env("NAN_BASE_URL", &self.endpoint)
            .env("NAN_API_KEY", "synthetic-native-reasoning-key")
            .env("NAN_NO_UPDATE_CHECK", "1")
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
            .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
            .env("LITELLM_LOCAL_MODEL_COST_MAP", "True")
            .env("NO_COLOR", "1")
            .env("CI", "1")
            .timeout(Duration::from_mins(1))
    }

    pub async fn configure(&self, harness: &str) {
        let output = self
            .command(env!("CARGO_BIN_EXE_nan-harness"))
            .args(["config", harness, "--yes", "--no-search"])
            .run()
            .await
            .unwrap();
        assert_success(&output);
    }

    pub fn managed(&self, harness: &str, model: &str) -> TerminalCommand {
        self.command(env!("CARGO_BIN_EXE_nan-harness"))
            .args([harness, "--model", model, "--executable"])
            .args([executable(harness)])
            .args(["--provider-base-url", &self.endpoint, "--no-search"])
    }

    pub fn remembered(&self, harness: &str, model: &str, selection: &Value) -> TerminalCommand {
        self.write_json(".nan-harness/preferences.json", &json!({
            "schemaVersion":3, "lastSelectionByHarness": {harness: {"model":model,"reasoning":selection}},
            "lastSelectionByDesktop":{}
        }));
        self.command(env!("CARGO_BIN_EXE_nan-harness"))
            .args([harness, "--executable"])
            .args([executable(harness)])
            .args(["--provider-base-url", &self.endpoint, "--no-search"])
    }

    pub fn captured(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    pub fn assert_effort(&self, model: &str, expected: Option<&str>) {
        let requests = self.captured();
        let matching = requests
            .iter()
            .filter(|request| request["model"] == model)
            .collect::<Vec<_>>();
        assert!(!matching.is_empty(), "no request for {model}: {requests:?}");
        // These callers use native coding requests with advertised tools.
        // OpenCode/MiMo use the exact-task selector below for their helpers.
        // Aider's text-only main request has no tools, so retain it when no
        // coding request exists. Do not let helper defaults satisfy assertions.
        let has_tools = |request: &&Value| {
            request["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
        };
        let main = if matching.iter().any(has_tools) {
            matching
                .iter()
                .copied()
                .filter(has_tools)
                .collect::<Vec<_>>()
        } else {
            matching.clone()
        };
        assert!(
            main.iter()
                .all(|request| request.get("reasoning_effort").and_then(Value::as_str) == expected),
            "{model} expected {expected:?}; main requests {:?}",
            main.iter()
                .map(|request| request.get("reasoning_effort"))
                .collect::<Vec<_>>()
        );
        assert!(
            matching
                .iter()
                .all(|request| request.get("reasoning").is_none()),
            "native effort must use the NaN top-level field"
        );
    }

    pub fn primary_chat_requests(&self, model: &str) -> Vec<Value> {
        self.captured()
            .into_iter()
            .filter(|request| {
                if request["model"] != model {
                    return false;
                }
                is_primary_chat_task(request)
            })
            .collect()
    }

    pub fn assert_chat_effort(&self, model: &str, expected: Option<&str>) {
        let main = self.primary_chat_requests(model);
        assert!(!main.is_empty(), "no primary chat task for {model}");
        for request in main {
            assert_eq!(
                request.get("reasoning_effort").and_then(Value::as_str),
                expected
            );
            assert!(request.get("reasoning").is_none());
        }
    }

    pub fn write_json(&self, relative: &str, value: &Value) {
        let path = self.home.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    pub fn read_json(&self, relative: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.home.join(relative)).unwrap()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

pub fn executable(harness: &str) -> PathBuf {
    let key = format!(
        "NAN_REASONING_{}_EXECUTABLE",
        harness.to_ascii_uppercase().replace('-', "_")
    );
    std::env::var_os(key).map_or_else(|| Path::new(harness).to_owned(), PathBuf::from)
}

async fn chat(
    State(requests): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    requests.lock().unwrap().push(body.clone());
    if body["stream"] == true {
        let chunks = [
            json!({"id":"reasoning-fixture","object":"chat.completion.chunk","created":1,
            "model":body["model"],"choices":[{"index":0,"delta":{"role":"assistant","content":MARKER},"finish_reason":null}]}),
            json!({"id":"reasoning-fixture","object":"chat.completion.chunk","created":1,"model":body["model"],
                "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
        ];
        let stream = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            chunks[0], chunks[1]
        );
        ([("content-type", "text/event-stream")], stream).into_response()
    } else {
        Json(json!({"id":"reasoning-fixture","object":"chat.completion","created":1,"model":body["model"],
            "choices":[{"index":0,"message":{"role":"assistant","content":MARKER},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})).into_response()
    }
}

fn normalized_text(content: &Value) -> String {
    let text = if let Some(text) = content.as_str() {
        text.to_owned()
    } else {
        content
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    };
    let text = text.trim();
    serde_json::from_str::<String>(text).unwrap_or_else(|_| text.to_owned())
}

fn is_primary_chat_task(request: &Value) -> bool {
    let Some(messages) = request["messages"].as_array() else {
        return false;
    };
    let users = messages
        .iter()
        .filter(|message| message["role"] == "user")
        .collect::<Vec<_>>();
    // Title helpers either add a user instruction or embed the task in a longer prompt.
    users.len() == 1 && normalized_text(&users[0]["content"]) == "Reply without tools."
}

#[test]
fn primary_task_matching_excludes_helpers_that_repeat_the_prompt() {
    let main = json!({"messages":[{"role":"user","content":"\"Reply without tools.\"\n"}]});
    let parts = json!({"messages":[{"role":"user","content":[{"type":"text","text":"Reply without tools."}]}]});
    let opencode_title = json!({"messages":[
        {"role":"user","content":"Generate a title for this conversation:"},
        {"role":"user","content":"\"Reply without tools.\""}
    ]});
    let mimo_title = json!({"messages":[{"role":"user","content":"Generate a title. Conversation: Reply without tools."}]});
    assert!(is_primary_chat_task(&main));
    assert!(is_primary_chat_task(&parts));
    assert!(!is_primary_chat_task(&opencode_title));
    assert!(!is_primary_chat_task(&mimo_title));
}
