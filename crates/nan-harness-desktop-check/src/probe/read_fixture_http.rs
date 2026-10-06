//! A lifecycle-owned MCP tool that reads the real fixture, never its oracle.

mod custody;
#[cfg(test)]
mod tests;

use super::{ProbeSpec, Reason};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse as _, Response},
    routing::post,
};
use custody::ReadTarget;
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

pub(super) const POLICY: &str = "NANH_CLAUDE_WINDOWS_MCP_FIXTURE";
pub(super) const ENDPOINT: &str = "NANH_CLAUDE_WINDOWS_MCP_URL";
const MAX_REQUESTS: usize = 64;
const PROTOCOLS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

struct ServerState {
    target: Mutex<ReadTarget>,
    authority: String,
    deadline: Instant,
    requests: AtomicUsize,
    initialized: AtomicUsize,
}

pub(super) struct ReadFixtureServer {
    pub(super) url: String,
    state: Arc<ServerState>,
    task: JoinHandle<()>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ReadFixtureServer {
    pub(super) async fn prepare(spec: &ProbeSpec, target: &Path) -> Result<Option<Self>, Reason> {
        if std::env::var_os(ENDPOINT).is_some() {
            return Err(Reason::IsolationUnavailable);
        }
        if std::env::var_os(POLICY).is_none() {
            return Ok(None);
        }
        if !cfg!(windows)
            || spec.live
            || spec.kind != nan_harness_core::DesktopHarnessKind::Claude
            || spec.session != crate::cli::SessionMode::GithubHosted
            || spec.verification != crate::cli::VerificationPolicy::SemanticOnly
            || [
                (POLICY, "read-only"),
                ("GITHUB_ACTIONS", "true"),
                ("RUNNER_ENVIRONMENT", "github-hosted"),
                ("RUNNER_OS", "Windows"),
                ("NANH_CLAUDE_WINDOWS_FRESH_PROFILE", "1"),
                ("NANH_CLAUDE_WINDOWS_NATIVE_CHAT", "1"),
                ("NANH_CLAUDE_WINDOWS_CHAT_ONLY", "1"),
                ("NANH_CLAUDE_WINDOWS_PROFILE_POLICY", "private-env"),
                ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
            ]
            .into_iter()
            .any(|(key, expected)| std::env::var(key).as_deref() != Ok(expected))
            || ["NANH_CLAUDE_MCP_FIXTURE", "NANH_CLAUDE_LINUX_MCP_FIXTURE"]
                .into_iter()
                .any(|key| std::env::var_os(key).is_some())
        {
            return Err(Reason::IsolationUnavailable);
        }
        Self::start(target, crate::runner::worker_timeout(false))
            .await
            .map(Some)
    }

    async fn start(target: &Path, lifetime: Duration) -> Result<Self, Reason> {
        let target = ReadTarget::open(target).map_err(|_| Reason::IsolationUnavailable)?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Reason::IsolationUnavailable)?;
        let authority = listener
            .local_addr()
            .map_err(|_| Reason::IsolationUnavailable)?
            .to_string();
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
        let mut encoded = String::with_capacity(32);
        for byte in nonce {
            write!(&mut encoded, "{byte:02x}").map_err(|_| Reason::IsolationUnavailable)?;
        }
        let path = format!("/mcp/{encoded}");
        let url = format!("http://{authority}{path}");
        let state = Arc::new(ServerState {
            target: Mutex::new(target),
            authority,
            deadline: Instant::now() + lifetime,
            requests: AtomicUsize::new(0),
            initialized: AtomicUsize::new(0),
        });
        let router = Router::new()
            .route(&path, post(request).get(unsupported_stream))
            .layer(DefaultBodyLimit::max(8192))
            .with_state(Arc::clone(&state));
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = tokio::time::timeout(lifetime, stopped).await;
                })
                .await;
        });
        Ok(Self {
            url,
            state,
            task,
            shutdown: Some(shutdown),
        })
    }

    pub(super) fn owns(&self, path: &Path) -> bool {
        !self.task.is_finished()
            && Instant::now() < self.state.deadline
            && self
                .state
                .target
                .lock()
                .is_ok_and(|target| target.owns(path))
    }

    pub(super) async fn stop(mut self) -> Result<(), Reason> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .map_err(|_| Reason::CleanupFailed)?
            .map_err(|_| Reason::CleanupFailed)
    }
}

impl Drop for ReadFixtureServer {
    fn drop(&mut self) {
        // The server runs in this worker, so no child listener can outlive it.
        self.task.abort();
    }
}

fn rpc_error(id: &Value, code: i32) -> Response {
    Json(
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":"Fixture request rejected"}}),
    )
    .into_response()
}

fn valid_headers(state: &ServerState, headers: &HeaderMap) -> bool {
    if headers.get("host").and_then(|value| value.to_str().ok()) != Some(state.authority.as_str())
        || headers.get("origin").is_some_and(|value| {
            value.to_str().ok() != Some(format!("http://{}", state.authority).as_str())
        })
        || headers.get("mcp-protocol-version").is_some_and(|value| {
            !value
                .to_str()
                .is_ok_and(|version| PROTOCOLS.contains(&version))
        })
    {
        return false;
    }
    true
}

async fn unsupported_stream(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> StatusCode {
    if valid_headers(&state, &headers) {
        StatusCode::METHOD_NOT_ALLOWED
    } else {
        StatusCode::BAD_REQUEST
    }
}

async fn request(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if Instant::now() >= state.deadline
        || state.requests.fetch_add(1, Ordering::SeqCst) >= MAX_REQUESTS
    {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !valid_headers(&state, &headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !body.is_object() {
        return rpc_error(&Value::Null, -32600);
    }
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(id) = body.get("id") else {
        return if method == "notifications/initialized"
            && state.initialized.load(Ordering::SeqCst) > 0
        {
            StatusCode::ACCEPTED.into_response()
        } else {
            StatusCode::BAD_REQUEST.into_response()
        };
    };
    if !id.is_i64()
        && !id.as_str().is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        })
    {
        return rpc_error(&Value::Null, -32600);
    }
    let result = match method {
        "initialize" => {
            let version = body
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| PROTOCOLS.contains(version))
                .unwrap_or(PROTOCOLS[0]);
            state.initialized.fetch_add(1, Ordering::SeqCst);
            json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"nanh-read-fixture","version":"1"}})
        }
        _ if state.initialized.load(Ordering::SeqCst) == 0 => return rpc_error(id, -32000),
        "ping" => json!({}),
        "tools/list" => {
            let Ok(target) = state.target.lock() else {
                return rpc_error(id, -32603);
            };
            json!({"tools":[{"name":"read_file","description":"Read the single owned qualification file", "inputSchema":{"type":"object","properties":{"path":{"type":"string","const":target.path()}},"required":["path"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}]})
        }
        "tools/call" => {
            let Ok(mut target) = state.target.lock() else {
                return rpc_error(id, -32603);
            };
            if body.pointer("/params/name").and_then(Value::as_str) != Some("read_file")
                || body.pointer("/params/arguments") != Some(&json!({"path":target.path()}))
            {
                return rpc_error(id, -32602);
            }
            match target.read() {
                Ok(text) => json!({"content":[{"type":"text","text":text}],"isError":false}),
                Err(_) => {
                    json!({"content":[{"type":"text","text":"Owned fixture is unavailable"}],"isError":true})
                }
            }
        }
        _ => return rpc_error(id, -32601),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}
