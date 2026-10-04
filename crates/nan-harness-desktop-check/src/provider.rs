//! Local boundary that keeps the real key out of application environments and bounds calls.

mod tool_result;
pub(crate) use tool_result::{SelectedTool, ToolResultObservation};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse as _, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
};
use std::time::Duration;
use subtle::ConstantTimeEq as _;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use zeroize::Zeroizing;

#[cfg(any(target_os = "macos", test))]
mod failure_turn;
#[cfg(target_os = "macos")]
pub(crate) use failure_turn::FailureAuthorityObservation;
#[cfg(any(target_os = "macos", test))]
use failure_turn::FailureTurnAuthority;

const MAX_GENERATIONS: usize = 4;
const MAX_OUTPUT_TOKENS: u64 = 2048;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
struct FixtureOracle {
    epoch: u64,
    marker: Option<String>,
    verified: bool,
}

impl FixtureOracle {
    fn arm(&mut self, marker: &str, live: bool) -> Result<(), ()> {
        if live || marker.is_empty() || marker.len() > 2048 {
            return Err(());
        }
        self.epoch = self.epoch.checked_add(1).ok_or(())?;
        self.marker = Some(marker.into());
        self.verified = false;
        Ok(())
    }

    fn snapshot(&self) -> Option<(u64, String)> {
        self.marker
            .as_ref()
            .map(|marker| (self.epoch, marker.clone()))
    }

    fn complete(&mut self, epoch: u64) {
        if self.epoch == epoch {
            self.verified = true;
        }
    }
}

struct GateState {
    client: reqwest::Client,
    upstream: Mutex<String>,
    key: Zeroizing<String>,
    session_token: Zeroizing<String>,
    live: bool,
    generations: AtomicUsize,
    expected_failure: AtomicBool,
    failure_observed: AtomicBool,
    #[cfg(any(target_os = "macos", test))]
    failure_turn: Mutex<FailureTurnAuthority>,
    unauthorized: AtomicBool,
    budget_exceeded: AtomicBool,
    tool_marker: String,
    tool_verified: AtomicBool,
    response_verified: AtomicBool,
    fixture_oracle: Mutex<FixtureOracle>,
    expected_failure_status: AtomicU16,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(crate) struct ClaudeFailureEpoch(u64);

pub(crate) struct ProviderGate {
    pub(crate) base_url: String,
    state: Arc<GateState>,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ProviderGate {
    pub(crate) async fn start(
        upstream: &str,
        key: Zeroizing<String>,
        live: bool,
        marker: &str,
    ) -> Result<Self, ()> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_mins(2))
            .build()
            .map_err(|_| ())?;
        let state = Arc::new(GateState {
            client,
            upstream: Mutex::new(upstream.trim_end_matches('/').into()),
            key,
            session_token: session_token()?,
            live,
            generations: AtomicUsize::new(0),
            expected_failure: AtomicBool::new(false),
            failure_observed: AtomicBool::new(false),
            #[cfg(any(target_os = "macos", test))]
            failure_turn: Mutex::new(FailureTurnAuthority::default()),
            unauthorized: AtomicBool::new(false),
            budget_exceeded: AtomicBool::new(false),
            tool_marker: marker.into(),
            tool_verified: AtomicBool::new(false),
            response_verified: AtomicBool::new(false),
            fixture_oracle: Mutex::new(FixtureOracle::default()),
            expected_failure_status: AtomicU16::new(400),
        });
        let router = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat))
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
            .layer(middleware::from_fn_with_state(
                Arc::clone(&state),
                authenticate,
            ))
            .with_state(Arc::clone(&state));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|_| ())?;
        let base_url = format!("http://{}/v1", listener.local_addr().map_err(|_| ())?);
        let (shutdown, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await;
        });
        Ok(Self {
            base_url,
            state,
            shutdown: Some(shutdown),
            task,
        })
    }

    pub(crate) fn use_upstream(&self, url: &str) {
        *self
            .state
            .upstream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = url.trim_end_matches('/').into();
    }

    pub(crate) fn session_token(&self) -> &str {
        self.state.session_token.as_str()
    }

    pub(crate) fn response_verified(&self) -> bool {
        self.state.response_verified.load(Ordering::SeqCst)
    }

    pub(crate) fn expect_fixture_response(&self, marker: &str) -> Result<(), ()> {
        let mut oracle = self
            .state
            .fixture_oracle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if oracle.marker.is_some() {
            return Err(());
        }
        oracle.arm(marker, self.state.live)
    }

    pub(crate) fn arm_fixture_response(&self, marker: &str) -> Result<(), ()> {
        self.state
            .fixture_oracle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm(marker, self.state.live)
    }

    pub(crate) fn reset_tool_verification(&self) {
        self.state.tool_verified.store(false, Ordering::SeqCst);
    }

    pub(crate) fn generation_count(&self) -> usize {
        self.state.generations.load(Ordering::SeqCst)
    }

    pub(crate) fn fixture_response_verified(&self) -> bool {
        self.state
            .fixture_oracle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .verified
    }

    pub(crate) fn fail_next_scenario(&self, enabled: bool) {
        self.state
            .expected_failure_status
            .store(400, Ordering::SeqCst);
        self.state.expected_failure.store(enabled, Ordering::SeqCst);
    }

    pub(crate) fn fail_recoverable_scenario(&self, enabled: bool) {
        self.state
            .expected_failure_status
            .store(503, Ordering::SeqCst);
        self.state.expected_failure.store(enabled, Ordering::SeqCst);
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn authorize_claude_fixture_failure(
        &self,
        selected: SelectedTool,
        owned: bool,
    ) -> Result<(), ()> {
        if !owned
            || !matches!(selected, SelectedTool::FixtureRead)
            || self.state.live
            || !self.tool_verified()
            || !self.fixture_response_verified()
            || [
                ("GITHUB_ACTIONS", "true"),
                ("RUNNER_ENVIRONMENT", "github-hosted"),
                ("RUNNER_OS", "macOS"),
                ("NANH_CLAUDE_MCP_FIXTURE", "read-only"),
                ("NANH_CLAUDE_MAC_PROFILE_POLICY", "native-known-folders"),
                ("NANH_CLAUDE_MAC_NATIVE_CHAT", "1"),
                ("NANH_CLAUDE_MAC_CHAT_NAVIGATION", "1"),
            ]
            .into_iter()
            .any(|(key, value)| std::env::var(key).as_deref() != Ok(value))
        {
            return Err(());
        }
        self.state
            .failure_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .authorize_fixture_context()
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn prepare_claude_turn(
        &self,
        prompt: &str,
        failure: bool,
    ) -> Result<Option<ClaudeFailureEpoch>, ()> {
        let mut authority = self
            .state
            .failure_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let epoch = authority.prepare(prompt, failure, self.state.live)?;
        if failure {
            // Arm the request-specific epoch before exposing an injected status;
            // no detached request can enter a legacy body-agnostic failure gap.
            self.state.failure_observed.store(false, Ordering::SeqCst);
            self.state
                .expected_failure_status
                .store(503, Ordering::SeqCst);
            self.state.expected_failure.store(true, Ordering::SeqCst);
        }
        Ok(epoch.map(ClaudeFailureEpoch))
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn claude_failure_authority_rejection(&self) -> FailureAuthorityObservation {
        self.state
            .failure_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .rejection_observation()
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn claude_failure_turn_observed(&self, epoch: ClaudeFailureEpoch) -> bool {
        self.state
            .failure_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed(epoch.0)
    }
    pub(crate) fn failure_observed(&self) -> bool {
        self.state.failure_observed.load(Ordering::SeqCst)
    }
    pub(crate) fn tool_verified(&self) -> bool {
        self.state.tool_verified.load(Ordering::SeqCst)
    }
    pub(crate) fn unauthorized(&self) -> bool {
        self.state.unauthorized.load(Ordering::SeqCst)
    }
    pub(crate) fn budget_exceeded(&self) -> bool {
        self.state.budget_exceeded.load(Ordering::SeqCst)
    }
}

fn session_token() -> Result<Zeroizing<String>, ()> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| ())?;
    Ok(Zeroizing::new(crate::report::digest(bytes.as_ref())))
}

async fn authenticate(
    State(state): State<Arc<GateState>>,
    request: Request,
    next: Next,
) -> Response {
    let mut values = request.headers().get_all(AUTHORIZATION).iter();
    let credential = values
        .next()
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.split_once(' '));
    let authorized = values.next().is_none()
        && credential.is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("bearer")
                && token.len() == state.session_token.len()
                && bool::from(token.as_bytes().ct_eq(state.session_token.as_bytes()))
        });
    if !authorized {
        return closed_error(StatusCode::UNAUTHORIZED, "NAN_CHECK_UNAUTHORIZED");
    }
    next.run(request).await
}

impl Drop for ProviderGate {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
    }
}

async fn models(State(state): State<Arc<GateState>>) -> Response {
    forward(&state, None).await
}

async fn chat(State(state): State<Arc<GateState>>, Json(mut body): Json<Value>) -> Response {
    #[cfg(any(target_os = "macos", test))]
    let inject_failure = {
        let mut authority = state
            .failure_turn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        authority.learn_context(&body);
        state.expected_failure.load(Ordering::SeqCst)
            && (!authority.armed() || authority.observe(&body))
    };
    #[cfg(not(any(target_os = "macos", test)))]
    let inject_failure = state.expected_failure.load(Ordering::SeqCst);
    if inject_failure {
        state.failure_observed.store(true, Ordering::SeqCst);
        let status = StatusCode::from_u16(state.expected_failure_status.load(Ordering::SeqCst))
            .unwrap_or(StatusCode::BAD_REQUEST);
        let mut response = closed_error(status, "NAN_CHECK_EXPECTED_FAILURE");
        if status == StatusCode::SERVICE_UNAVAILABLE {
            response.headers_mut().insert(
                "x-should-retry",
                axum::http::HeaderValue::from_static("false"),
            );
        }
        return response;
    }
    let count = state.generations.fetch_add(1, Ordering::SeqCst);
    if state.live && count >= MAX_GENERATIONS {
        state.budget_exceeded.store(true, Ordering::SeqCst);
        return closed_error(StatusCode::TOO_MANY_REQUESTS, "NAN_CHECK_BUDGET_EXCEEDED");
    }
    if tool_contains_marker(&body, &state.tool_marker) {
        state.tool_verified.store(true, Ordering::SeqCst);
    }
    if state.live {
        if let Some(object) = body.as_object_mut() {
            let key = if object.contains_key("max_completion_tokens") {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            let limit = object
                .get(key)
                .and_then(Value::as_u64)
                .unwrap_or(MAX_OUTPUT_TOKENS)
                .min(MAX_OUTPUT_TOKENS);
            object.insert(key.into(), json!(limit));
            for alternate in ["max_tokens", "max_completion_tokens"] {
                if let Some(value) = object.get_mut(alternate) {
                    *value = json!(
                        value
                            .as_u64()
                            .unwrap_or(MAX_OUTPUT_TOKENS)
                            .min(MAX_OUTPUT_TOKENS)
                    );
                }
            }
            if object.contains_key("n") {
                object.insert("n".into(), json!(1));
            }
        } else {
            return closed_error(StatusCode::BAD_REQUEST, "NAN_CHECK_INVALID_REQUEST");
        }
    }
    forward(&state, Some(body)).await
}

fn tool_contains_marker(body: &Value, marker: &str) -> bool {
    body.get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("tool")
                    && message
                        .get("content")
                        .is_some_and(|content| content.to_string().contains(marker))
            })
        })
}

async fn forward(state: &GateState, body: Option<Value>) -> Response {
    let generation = body.is_some();
    let fixture = state
        .fixture_oracle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .snapshot();
    let upstream = state
        .upstream
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let request = if let Some(body) = body {
        state
            .client
            .post(format!("{upstream}/chat/completions"))
            .json(&body)
    } else {
        state.client.get(format!("{upstream}/models"))
    };
    let result = request.bearer_auth(state.key.as_str()).send().await;
    let Ok(mut response) = result else {
        return closed_error(StatusCode::BAD_GATEWAY, "NAN_CHECK_PROVIDER_UNAVAILABLE");
    };
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        state.unauthorized.store(true, Ordering::SeqCst);
    }
    let mut builder = Response::builder().status(status);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .cloned();
    if let Some(content_type) = &content_type {
        builder = builder.header(axum::http::header::CONTENT_TYPE, content_type);
    }
    // The checker buffers a bounded response so incomplete streams cannot be
    // certified or shown as complete. The original JSON/SSE bytes are preserved.
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Ok(Some(_)) | Err(_) => {
                return closed_error(StatusCode::BAD_GATEWAY, "NAN_CHECK_INVALID_RESPONSE");
            }
        }
    }
    if generation
        && state.live
        && state.tool_verified.load(Ordering::SeqCst)
        && status.is_success()
        && completed_response(
            &bytes,
            content_type.as_ref().and_then(|value| value.to_str().ok()),
            &format!("NAN_CHECK_FINAL:{}", state.tool_marker),
        )
    {
        state.response_verified.store(true, Ordering::SeqCst);
    }
    if generation
        && !state.live
        && status.is_success()
        && let Some((epoch, marker)) = fixture
        && completed_response(
            &bytes,
            content_type.as_ref().and_then(|value| value.to_str().ok()),
            &marker,
        )
    {
        state
            .fixture_oracle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .complete(epoch);
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| closed_error(StatusCode::BAD_GATEWAY, "NAN_CHECK_PROVIDER_UNAVAILABLE"))
}

fn completed_response(bytes: &[u8], content_type: Option<&str>, marker: &str) -> bool {
    if content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
    }) {
        return completed_stream(bytes, marker);
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    value.get("error").is_none()
        && value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| {
                choices.len() == 1
                    && choices.iter().any(|choice| {
                        choice
                            .get("index")
                            .is_none_or(|index| index.as_u64() == Some(0))
                            && choice.get("finish_reason").and_then(Value::as_str) == Some("stop")
                            && choice.pointer("/message/role").and_then(Value::as_str)
                                == Some("assistant")
                            && choice
                                .pointer("/message/content")
                                .and_then(Value::as_str)
                                .is_some_and(|content| content.contains(marker))
                    })
            })
}

#[derive(Default)]
struct StreamObservation {
    content: String,
    assistant: bool,
    finished: bool,
    done: bool,
}

fn completed_stream(bytes: &[u8], marker: &str) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.replace("\r\n", "\n");
    if !text.ends_with("\n\n") {
        return false;
    }
    let mut observation = StreamObservation::default();
    for event in text.split("\n\n") {
        if event.lines().any(|line| {
            line.strip_prefix("event:")
                .is_some_and(|kind| kind.trim() == "error")
        }) {
            return false;
        }
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if observation.done {
            return false;
        }
        if data == "[DONE]" {
            observation.done = true;
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            return false;
        };
        if !observe_delta(&mut observation, &value) {
            return false;
        }
    }
    observation.done
        && observation.finished
        && observation.assistant
        && observation.content.contains(marker)
}

fn observe_delta(observation: &mut StreamObservation, value: &Value) -> bool {
    if value.get("error").is_some() {
        return false;
    }
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return false;
    };
    if choices.len() > 1 {
        return false;
    }
    for choice in choices {
        if !choice.get("delta").is_some_and(Value::is_object) {
            return false;
        }
        if choice
            .get("index")
            .is_some_and(|index| index.as_u64() != Some(0))
        {
            return false;
        }
        if let Some(role) = choice.pointer("/delta/role").and_then(Value::as_str) {
            if role != "assistant" {
                return false;
            }
            observation.assistant = true;
        }
        if let Some(content) = choice.pointer("/delta/content").and_then(Value::as_str) {
            if observation.finished
                || observation.content.len().saturating_add(content.len()) > MAX_RESPONSE_BYTES
            {
                return false;
            }
            observation.content.push_str(content);
            // Chat Completions deltas are assistant output. Existing provider
            // streams can omit the redundant role, but an explicit other role is rejected.
            observation.assistant = true;
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            if reason != "stop" || observation.finished {
                return false;
            }
            observation.finished = true;
        }
    }
    true
}

fn closed_error(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        Json(json!({"error":{"type":"desktop_check", "message":code}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_or_assistant_claim_is_not_tool_evidence() {
        for role in ["user", "assistant", "system"] {
            assert!(!tool_contains_marker(
                &json!({"messages":[{"role":role,"content":"secret-marker"}]}),
                "secret-marker"
            ));
        }
        assert!(tool_contains_marker(
            &json!({"messages":[{"role":"tool","content":"secret-marker"}]}),
            "secret-marker"
        ));
    }

    #[tokio::test]
    async fn failure_is_explicit_and_generation_budget_prevents_fifth_forward() {
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("private-key".into()),
            true,
            "marker",
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        gate.fail_next_scenario(true);
        let response = client
            .post(format!("{}/chat/completions", gate.base_url))
            .bearer_auth(gate.session_token())
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(gate.failure_observed());
        assert!(response.headers().get("x-should-retry").is_none());
        assert!(!response.text().await.unwrap().contains("private-key"));
        gate.state
            .expected_failure_status
            .store(503, Ordering::SeqCst);
        let response = client
            .post(format!("{}/chat/completions", gate.base_url))
            .bearer_auth(gate.session_token())
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("x-should-retry").unwrap(), "false");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["type"], "desktop_check");
        assert_eq!(body["error"]["message"], "NAN_CHECK_EXPECTED_FAILURE");
        gate.fail_next_scenario(false);
        gate.state.generations.store(4, Ordering::SeqCst);
        let response = client
            .post(format!("{}/chat/completions", gate.base_url))
            .bearer_auth(gate.session_token())
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(gate.budget_exceeded());
    }

    #[tokio::test]
    async fn unauthenticated_requests_cannot_forward_or_change_evidence() {
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("private-key".into()),
            true,
            "marker",
        )
        .await
        .unwrap();
        let other = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("private-key".into()),
            true,
            "marker",
        )
        .await
        .unwrap();
        assert_ne!(gate.session_token(), other.session_token());
        let client = reqwest::Client::new();
        gate.fail_next_scenario(true);
        for path in ["models", "chat/completions"] {
            let request = if path == "models" {
                client.get(format!("{}/{path}", gate.base_url))
            } else {
                client
                    .post(format!("{}/{path}", gate.base_url))
                    .body("invalid json")
            };
            assert_eq!(
                request.send().await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
        }
        for credential in ["nanh-desktop-check-session", other.session_token()] {
            let response = client
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(credential)
                .json(&json!({"messages":[{"role":"tool","content":"marker"}]}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let mut headers = axum::http::HeaderMap::new();
        headers.append(
            AUTHORIZATION,
            format!("Bearer {}", gate.session_token()).parse().unwrap(),
        );
        headers.append(AUTHORIZATION, "Bearer other".parse().unwrap());
        assert_eq!(
            client
                .get(format!("{}/models", gate.base_url))
                .headers(headers)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(gate.state.generations.load(Ordering::SeqCst), 0);
        assert!(!gate.failure_observed());
        assert!(!gate.tool_verified());
        assert!(!gate.unauthorized());
        assert!(!gate.budget_exceeded());
    }

    async fn upstream(
        status: StatusCode,
        content_type: &'static str,
        body: String,
    ) -> (String, JoinHandle<()>) {
        let router = Router::new().fallback(move |headers: axum::http::HeaderMap| {
            let body = body.clone();
            async move {
                assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer private-key");
                (
                    status,
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    body,
                )
            }
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (url, task)
    }

    fn final_json() -> String {
        json!({"choices":[{"index":0,"message":{"role":"assistant","content":"NAN_CHECK_FINAL:marker"},"finish_reason":"stop"}]}).to_string()
    }

    #[tokio::test]
    async fn rearmed_same_marker_rejects_in_flight_old_completion() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let router = Router::new().fallback({
            let entered = entered.clone();
            let release = release.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                async move {
                    entered.notify_one();
                    release.notified().await;
                    (
                        StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        final_json(),
                    )
                }
            }
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let gate = ProviderGate::start(&url, Zeroizing::new("private-key".into()), false, "marker")
            .await
            .unwrap();
        gate.arm_fixture_response("NAN_CHECK_FINAL:marker").unwrap();
        let request = || {
            reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"messages":[]}))
        };
        let pending = tokio::spawn(request().send());
        tokio::time::timeout(Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        gate.arm_fixture_response("NAN_CHECK_FINAL:marker").unwrap();
        release.notify_one();
        assert_eq!(pending.await.unwrap().unwrap().status(), StatusCode::OK);
        assert!(!gate.fixture_response_verified());
        release.notify_one();
        assert_eq!(request().send().await.unwrap().status(), StatusCode::OK);
        assert!(gate.fixture_response_verified());
        gate.arm_fixture_response("NAN_CHECK_FINAL:new-marker")
            .unwrap();
        assert!(!gate.fixture_response_verified());
        server.abort();
    }

    #[tokio::test]
    async fn recoverable_failure_is_observed_before_forwarded_recovery() {
        let (url, server) = upstream(StatusCode::OK, "application/json", final_json()).await;
        let gate = ProviderGate::start(&url, Zeroizing::new("private-key".into()), false, "marker")
            .await
            .unwrap();
        gate.arm_fixture_response("NAN_CHECK_FINAL:marker").unwrap();
        let request = || {
            reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"messages":[]}))
        };
        gate.fail_recoverable_scenario(true);
        let failure = request().send().await.unwrap();
        assert_eq!(failure.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            failure.json::<Value>().await.unwrap()["error"]["message"],
            "NAN_CHECK_EXPECTED_FAILURE"
        );
        assert!(gate.failure_observed());
        assert_eq!(gate.generation_count(), 0);
        assert!(!gate.fixture_response_verified());
        gate.fail_recoverable_scenario(false);
        assert_eq!(request().send().await.unwrap().status(), StatusCode::OK);
        assert_eq!(gate.generation_count(), 1);
        assert!(gate.fixture_response_verified());
        server.abort();
    }

    #[tokio::test]
    async fn tool_reset_requires_new_tool_role_evidence() {
        let gate = ProviderGate::start(
            "http://127.0.0.1:1/v1",
            Zeroizing::new("key".into()),
            false,
            "tool-marker",
        )
        .await
        .unwrap();
        for (role, verified) in [("tool", true), ("user", false), ("tool", true)] {
            if role == "user" {
                gate.reset_tool_verification();
                assert!(!gate.tool_verified());
            }
            let _ = reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"messages":[{"role":role,"content":"tool-marker"}]}))
                .send()
                .await
                .unwrap();
            assert_eq!(gate.tool_verified(), verified);
        }
    }

    #[tokio::test]
    async fn fixture_oracle_requires_authenticated_completed_generation() {
        let unfinished = "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"NAN_CHECK_FINAL:marker\"},\"finish_reason\":\"stop\"}]}\n\n";
        for (status, mime, body, valid) in [
            (StatusCode::OK, "application/json", final_json(), true),
            (
                StatusCode::OK,
                "application/json",
                final_json().replace("FINAL:marker", "FINAL:wrong"),
                false,
            ),
            (
                StatusCode::OK,
                "application/json",
                final_json().replace("stop", "length"),
                false,
            ),
            (
                StatusCode::OK,
                "text/event-stream",
                unfinished.to_owned(),
                false,
            ),
            (
                StatusCode::OK,
                "text/event-stream",
                format!("{unfinished}data: [DONE]\n\n"),
                true,
            ),
            (
                StatusCode::BAD_GATEWAY,
                "application/json",
                final_json(),
                false,
            ),
        ] {
            let (url, task) = upstream(status, mime, body).await;
            let gate =
                ProviderGate::start(&url, Zeroizing::new("private-key".into()), false, "marker")
                    .await
                    .unwrap();
            assert!(
                gate.expect_fixture_response("NAN_CHECK_FINAL:marker")
                    .is_ok()
            );
            assert!(gate.expect_fixture_response("another").is_err());
            let client = reqwest::Client::new();
            let _ = client
                .get(format!("{}/models", gate.base_url))
                .bearer_auth(gate.session_token())
                .send()
                .await
                .unwrap();
            assert!(!gate.fixture_response_verified());
            let unauthorized = client
                .post(format!("{}/chat/completions", gate.base_url))
                .json(&json!({"messages":[]}))
                .send()
                .await
                .unwrap();
            assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
            assert!(!gate.fixture_response_verified());
            let _ = client
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"messages":[]}))
                .send()
                .await
                .unwrap();
            assert_eq!(gate.fixture_response_verified(), valid);
            assert!(!gate.response_verified());
            assert!(!gate.tool_verified());
            task.abort();
        }
    }

    #[tokio::test]
    async fn fixture_expectation_refuses_live_and_invalid_markers() {
        for live in [false, true] {
            let gate = ProviderGate::start(
                "http://127.0.0.1:1/v1",
                Zeroizing::new("key".into()),
                live,
                "marker",
            )
            .await
            .unwrap();
            assert!(gate.expect_fixture_response("").is_err());
            assert!(gate.expect_fixture_response(&"x".repeat(2049)).is_err());
            assert!(gate.arm_fixture_response("").is_err());
            assert!(gate.arm_fixture_response(&"x".repeat(2049)).is_err());
            assert_eq!(gate.expect_fixture_response("fixture").is_ok(), !live);
            assert_eq!(gate.arm_fixture_response("rearmed").is_ok(), !live);
            assert!(!gate.fixture_response_verified());
        }
    }

    #[tokio::test]
    async fn successful_json_requires_tool_evidence_and_preserves_the_response() {
        let body = final_json();
        let (url, task) = upstream(StatusCode::OK, "application/json", body.clone()).await;
        let gate = ProviderGate::start(&url, Zeroizing::new("private-key".into()), true, "marker")
            .await
            .unwrap();
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/models", gate.base_url))
            .bearer_auth(gate.session_token())
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), body);
        assert!(!gate.response_verified());
        let response = client
            .post(format!("{}/chat/completions", gate.base_url))
            .bearer_auth(gate.session_token())
            .json(&json!({"messages":[{"role":"tool","content":"marker"}]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), body);
        assert!(gate.response_verified());
        task.abort();
    }

    #[tokio::test]
    async fn errors_malformed_and_incomplete_responses_never_certify() {
        let unfinished = "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"NAN_CHECK_FINAL:marker\"},\"finish_reason\":\"stop\"}]}\n\n";
        for (status, content_type, body) in [
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "application/json",
                final_json(),
            ),
            (StatusCode::OK, "application/json", "malformed".into()),
            (
                StatusCode::OK,
                "application/json",
                final_json().replace("stop", "length"),
            ),
            (StatusCode::OK, "text/event-stream", unfinished.into()),
        ] {
            let (url, task) = upstream(status, content_type, body).await;
            let gate =
                ProviderGate::start(&url, Zeroizing::new("private-key".into()), true, "marker")
                    .await
                    .unwrap();
            reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"messages":[{"role":"tool","content":"marker"}]}))
                .send()
                .await
                .unwrap();
            assert!(!gate.response_verified());
            task.abort();
        }
    }

    #[test]
    fn streaming_marker_must_be_in_a_finished_assistant_choice() {
        let stream = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"NAN_CHECK_\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"FINAL:marker\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        assert!(completed_response(
            stream.as_bytes(),
            Some("text/event-stream; charset=utf-8"),
            "NAN_CHECK_FINAL:marker"
        ));
        let implicit_assistant = stream
            .replace("\"role\":\"assistant\",", "")
            .replace("\"index\":0,", "");
        assert!(completed_response(
            implicit_assistant.as_bytes(),
            Some("text/event-stream"),
            "NAN_CHECK_FINAL:marker"
        ));
        for invalid in [
            stream.replace("assistant", "tool"),
            stream.replace("\"stop\"", "\"length\""),
            stream.replace("[DONE]", "{malformed}"),
            stream.replace("FINAL:marker", "marker"),
            format!("{stream}data: {{\"error\":{{}}}}\n\n"),
        ] {
            assert!(!completed_response(
                invalid.as_bytes(),
                Some("text/event-stream"),
                "NAN_CHECK_FINAL:marker"
            ));
        }
    }

    #[tokio::test]
    async fn oversized_responses_fail_closed() {
        let (url, task) = upstream(
            StatusCode::OK,
            "application/json",
            "x".repeat(MAX_RESPONSE_BYTES + 1),
        )
        .await;
        let gate = ProviderGate::start(&url, Zeroizing::new("private-key".into()), true, "marker")
            .await
            .unwrap();
        let response = reqwest::Client::new()
            .get(format!("{}/models", gate.base_url))
            .bearer_auth(gate.session_token())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(!gate.response_verified());
        task.abort();
    }

    #[tokio::test]
    async fn live_budget_caps_forwarded_generations_and_output_aliases() {
        let forwarded = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&forwarded);
        let router = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let observed = Arc::clone(&observed);
                async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["n"], 1);
                    assert_eq!(body["max_tokens"], 999);
                    assert_eq!(body["max_completion_tokens"], MAX_OUTPUT_TOKENS);
                    Json(json!({"choices":[]}))
                }
            }),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let gate = ProviderGate::start(&url, Zeroizing::new("private-key".into()), true, "marker")
            .await
            .unwrap();
        let client = reqwest::Client::new();
        for index in 0..5 {
            let response = client
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&json!({"n":99,"max_tokens":999,"max_completion_tokens":99999}))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if index < 4 {
                    StatusCode::OK
                } else {
                    StatusCode::TOO_MANY_REQUESTS
                }
            );
        }
        assert_eq!(forwarded.load(Ordering::SeqCst), MAX_GENERATIONS);
        assert!(gate.budget_exceeded());
        task.abort();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn fixture_tool_authority_injects_only_the_full_current_main_request() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(|| async { (StatusCode::OK, final_json()) }),
            )
            .await
            .unwrap();
        });
        let gate = ProviderGate::start(&upstream, Zeroizing::new("key".into()), false, "tool")
            .await
            .unwrap();
        let body = |prompts: &[&str], context: &str| {
            let mut messages = vec![json!({"role":"system","content":context})];
            messages.extend(
                prompts
                    .iter()
                    .map(|prompt| json!({"role":"user","content":prompt})),
            );
            json!({"model":"fixture","stream":true,"messages":messages,"tools":[{"type":"function","function":{"name":"mcp__nanh-read-fixture__read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]})
        };
        let post = |value: Value| {
            reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&value)
        };
        for (index, prompt) in ["first", "second"].iter().enumerate() {
            gate.prepare_claude_turn(prompt, false).unwrap();
            assert_eq!(
                post(body(&["first", "second"][..=index], "prior"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        // Tests the source-bound request discriminator independently of GUI policy admission.
        gate.state
            .failure_turn
            .lock()
            .unwrap()
            .authorize_fixture_context()
            .unwrap();
        let epoch = gate.prepare_claude_turn("third", true).unwrap().unwrap();
        assert_eq!(
            post(body(&["third"], "title"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let mut wrong = body(&["first", "second", "third"], "changed");
        wrong["tools"] = json!([]);
        assert_eq!(post(wrong).send().await.unwrap().status(), StatusCode::OK);
        assert!(!gate.claude_failure_turn_observed(epoch));
        let valid = body(&["first", "second", "third"], "changed");
        assert_eq!(
            post(valid.clone()).send().await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(gate.claude_failure_turn_observed(epoch));
        assert_eq!(post(valid).send().await.unwrap().status(), StatusCode::OK);
        server.abort();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn claude_failure_authority_forwards_background_and_consumes_one_current_error() {
        let router = Router::new().fallback(|| async {
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                final_json(),
            )
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let gate = ProviderGate::start(&url, Zeroizing::new("key".into()), false, "tool")
            .await
            .unwrap();
        let body = |prompts: &[&str], system: &str| {
            let mut messages = vec![json!({"role":"system","content":system})];
            for prompt in prompts {
                messages.push(json!({"role":"user","content":prompt}));
            }
            json!({"stream":true,"model":"fixture","messages":messages})
        };
        let post = |value: Value| {
            reqwest::Client::new()
                .post(format!("{}/chat/completions", gate.base_url))
                .bearer_auth(gate.session_token())
                .json(&value)
        };
        for (index, prompt) in ["response nonce1", "tool nonce2"].iter().enumerate() {
            assert!(gate.prepare_claude_turn(prompt, false).unwrap().is_none());
            assert_eq!(
                post(body(
                    &["response nonce1", "tool nonce2"][..=index],
                    if index == 0 { "initial" } else { "main" },
                ))
                .send()
                .await
                .unwrap()
                .status(),
                StatusCode::OK
            );
        }
        let mut nonstreaming = body(&["response nonce1", "tool nonce2"], "main");
        nonstreaming["stream"] = json!(false);
        assert_eq!(
            post(nonstreaming).send().await.unwrap().status(),
            StatusCode::OK
        );
        let observation = serde_json::to_value(gate.claude_failure_authority_rejection()).unwrap();
        assert_eq!(observation["rejectedStream"], 1);
        assert_eq!(observation["learnedTurns"], 2);
        assert_eq!(observation["rejectedContext"], 0);
        let epoch = gate
            .prepare_claude_turn("failure nonce3", true)
            .unwrap()
            .unwrap();
        assert_eq!(
            post(body(&["failure nonce3"], "title"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(!gate.failure_observed());
        assert!(!gate.claude_failure_turn_observed(epoch));
        let current = body(
            &["response nonce1", "tool nonce2", "failure nonce3"],
            "main",
        );
        let failure = post(current.clone()).send().await.unwrap();
        assert_eq!(failure.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(failure.headers().get("x-should-retry").unwrap(), "false");
        assert_eq!(
            failure.json::<Value>().await.unwrap()["error"]["message"],
            "NAN_CHECK_EXPECTED_FAILURE"
        );
        assert!(gate.failure_observed());
        assert!(gate.claude_failure_turn_observed(epoch));
        assert_eq!(gate.generation_count(), 4);
        assert_eq!(post(current).send().await.unwrap().status(), StatusCode::OK);
        assert_eq!(gate.generation_count(), 5);
        server.abort();
    }
}
