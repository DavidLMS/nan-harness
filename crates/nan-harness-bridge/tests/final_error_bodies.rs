use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use nan_harness_bridge::{
    BridgeActivity, BridgeConfig, BridgeEndpoint, ClaudeModelCatalog, CodexModelCatalog,
    FxGatewayConfig, FxModelCatalog, ResponsesBridgeConfig, RunningBridge,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const ERROR_BODY_LIMIT: usize = 64 * 1024;
const SESSION_TOKEN: &str = "session-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BridgeKind {
    Anthropic,
    Responses,
    FxGateway,
}

impl BridgeKind {
    const ALL: [Self; 3] = [Self::Anthropic, Self::Responses, Self::FxGateway];

    const fn endpoint(self) -> BridgeEndpoint {
        match self {
            Self::Anthropic => BridgeEndpoint::Messages,
            Self::Responses => BridgeEndpoint::Responses,
            Self::FxGateway => BridgeEndpoint::FxGateway,
        }
    }
}

#[derive(Clone)]
enum ProviderBody {
    Complete(Arc<[u8]>),
    Chunked(Arc<[u8]>),
    Progressing,
}

#[derive(Clone)]
struct ProviderState {
    status: StatusCode,
    body: ProviderBody,
    attempts: Arc<AtomicUsize>,
    consumed: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct TestSystem {
    bridge: RunningBridge,
    provider_task: tokio::task::JoinHandle<()>,
    state: ProviderState,
    kind: BridgeKind,
}

impl TestSystem {
    async fn request_error(&self) -> (StatusCode, String) {
        let client = reqwest::Client::new();
        let response = match self.kind {
            BridgeKind::Anthropic => {
                client
                    .post(format!("{}/v1/messages", self.bridge.base_url()))
                    .bearer_auth(SESSION_TOKEN)
                    .json(&json!({
                        "model": "anthropic/nan/qwen3.6",
                        "max_tokens": 128,
                        "messages": [{"role": "user", "content": "fail safely"}]
                    }))
                    .send()
                    .await
            }
            BridgeKind::Responses => {
                client
                    .post(format!("{}/v1/responses", self.bridge.base_url()))
                    .bearer_auth(SESSION_TOKEN)
                    .json(&responses_request())
                    .send()
                    .await
            }
            BridgeKind::FxGateway => {
                client
                    .post(format!("{}/v3/ai/language-model", self.bridge.base_url()))
                    .bearer_auth(SESSION_TOKEN)
                    .header("ai-language-model-id", "qwen3.6")
                    .json(&json!({
                        "prompt": [{"role": "user", "content": "fail safely"}],
                        "tools": []
                    }))
                    .send()
                    .await
            }
        }
        .expect("bridge request should receive an error response");
        let status = response.status();
        let body = response.text().await.expect("readable bridge error");
        (status, body)
    }

    async fn request_auto_error(&self) -> (StatusCode, String) {
        let response = reqwest::Client::new()
            .post(format!("{}/v1/messages", self.bridge.base_url()))
            .bearer_auth(SESSION_TOKEN)
            .json(&json!({
                "model": "opus",
                "max_tokens": 64,
                "temperature": 1,
                "thinking": {"type": "enabled", "budget_tokens": 1024},
                "system": [{
                    "type": "text",
                    "text": concat!(
                        "You are a security monitor for autonomous AI coding agents.\n",
                        "## Classification Process\n",
                        "## Output Format"
                    )
                }],
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "text",
                        "text": "Stage 1 does NOT apply user intent or ALLOW exceptions"
                    }]
                }]
            }))
            .send()
            .await
            .expect("Auto mode request should receive an error response");
        let status = response.status();
        let body = response.text().await.expect("Auto mode error body");
        (status, body)
    }

    async fn shutdown(mut self) {
        self.bridge.shutdown();
        self.bridge.wait().await.expect("clean bridge shutdown");
        self.provider_task.abort();
    }
}

#[tokio::test]
async fn translating_bridges_preserve_small_terminal_400_and_401_errors() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::UNAUTHORIZED] {
        for kind in BridgeKind::ALL {
            let payload = provider_error("specific detail\r\nsecond line");
            let mut system = start_system(kind, status, ProviderBody::Complete(payload)).await;
            let mut diagnostics = system.bridge.take_diagnostics();

            let (visible_status, body) = system.request_error().await;

            assert_visible_contract(kind, status, visible_status, &body);
            assert!(
                body.contains("specific detail  second line"),
                "{kind:?}: {body}"
            );
            let diagnostic = diagnostics.recv().await.expect("terminal diagnostic");
            assert_eq!(diagnostic.endpoint, kind.endpoint());
            assert_eq!(diagnostic.http_status, Some(status.as_u16()));
            system.shutdown().await;
        }
    }
}

#[tokio::test]
async fn translating_bridges_discard_oversized_terminal_error_bodies() {
    for kind in BridgeKind::ALL {
        let payload = padded_error("oversized-private-marker", ERROR_BODY_LIMIT + 1);
        let system = start_system(
            kind,
            StatusCode::BAD_REQUEST,
            ProviderBody::Chunked(payload.into()),
        )
        .await;

        let (visible_status, body) = system.request_error().await;

        assert_visible_contract(kind, StatusCode::BAD_REQUEST, visible_status, &body);
        assert!(body.contains("NaN request failed"), "{kind:?}: {body}");
        assert!(
            !body.contains("oversized-private-marker"),
            "{kind:?}: {body}"
        );
        assert!(system.state.dropped.load(Ordering::SeqCst));
        assert!(system.state.consumed.load(Ordering::SeqCst) <= ERROR_BODY_LIMIT + 4 * 1024);
        system.shutdown().await;
    }
}

#[tokio::test]
async fn exhausted_429_and_503_responses_keep_status_based_classification() {
    for (kind, status) in [
        (BridgeKind::Anthropic, StatusCode::TOO_MANY_REQUESTS),
        (BridgeKind::Responses, StatusCode::SERVICE_UNAVAILABLE),
        (BridgeKind::FxGateway, StatusCode::TOO_MANY_REQUESTS),
    ] {
        let system = start_system(
            kind,
            status,
            ProviderBody::Complete(provider_error("retry budget exhausted")),
        )
        .await;

        let (visible_status, body) = system.request_error().await;

        assert_visible_contract(kind, status, visible_status, &body);
        assert!(body.contains("retry budget exhausted"), "{kind:?}: {body}");
        if status == StatusCode::TOO_MANY_REQUESTS && kind == BridgeKind::Anthropic {
            assert!(body.contains("rate_limit_error"), "{body}");
        }
        assert_eq!(system.state.attempts.load(Ordering::SeqCst), 3);
        system.shutdown().await;
    }
}

#[tokio::test]
async fn progressing_error_body_hits_an_absolute_deadline() {
    let system = start_system(
        BridgeKind::Responses,
        StatusCode::BAD_REQUEST,
        ProviderBody::Progressing,
    )
    .await;
    let started = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(3), system.request_error()).await;

    let (status, body) = result.expect("absolute body deadline should beat the test deadline");
    assert_visible_contract(
        BridgeKind::Responses,
        StatusCode::BAD_REQUEST,
        status,
        &body,
    );
    assert!(body.contains("NaN request failed"), "{body}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(system.state.consumed.load(Ordering::SeqCst) > 1);
    assert!(system.state.dropped.load(Ordering::SeqCst));
    system.shutdown().await;
}

#[tokio::test]
async fn auto_mode_trace_reports_only_complete_error_bodies_as_responses() {
    for (body, expected_marker, expect_response) in [
        (
            ProviderBody::Complete(provider_error("complete-trace-marker")),
            "complete-trace-marker",
            true,
        ),
        (
            ProviderBody::Chunked(
                padded_error("incomplete-trace-marker", ERROR_BODY_LIMIT + 1).into(),
            ),
            "NaN request failed",
            false,
        ),
    ] {
        let system = start_system_with_auto_traces(
            BridgeKind::Anthropic,
            StatusCode::BAD_REQUEST,
            body,
            true,
        )
        .await;
        let mut activities = system.bridge.subscribe_activities();

        let (status, error_body) = system.request_auto_error().await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error_body.contains(expected_marker), "{error_body}");
        let mut saw_review = false;
        let mut saw_response = false;
        let mut saw_failure = false;
        while let Ok(activity) = activities.try_recv() {
            match activity {
                BridgeActivity::ClaudeAutoModeReview { .. } => saw_review = true,
                BridgeActivity::ClaudeAutoModeReviewResponse { response, .. } => {
                    saw_response = true;
                    assert!(
                        response.with_contents(|payload| payload.contains("complete-trace-marker"))
                    );
                }
                BridgeActivity::ClaudeAutoModeReviewFailed { error_code, .. } => {
                    saw_failure = true;
                    assert_eq!(error_code, "NH-BRIDGE-104");
                }
                BridgeActivity::AuthenticatedClient => {}
            }
        }
        assert!(saw_review);
        assert_eq!(saw_response, expect_response);
        assert_eq!(saw_failure, !expect_response);
        system.shutdown().await;
    }
}

fn assert_visible_contract(
    kind: BridgeKind,
    upstream_status: StatusCode,
    visible_status: StatusCode,
    body: &str,
) {
    let expected_status = match kind {
        BridgeKind::Responses => StatusCode::OK,
        _ if upstream_status == StatusCode::TOO_MANY_REQUESTS => StatusCode::TOO_MANY_REQUESTS,
        _ if upstream_status.is_client_error() => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_GATEWAY,
    };
    assert_eq!(visible_status, expected_status, "{kind:?}: {body}");
    assert!(body.contains("NH-BRIDGE-104"), "{kind:?}: {body}");
    if kind == BridgeKind::Responses {
        assert!(body.contains("event: response.failed"), "{body}");
    }
}

async fn start_system(kind: BridgeKind, status: StatusCode, body: ProviderBody) -> TestSystem {
    start_system_with_auto_traces(kind, status, body, false).await
}

async fn start_system_with_auto_traces(
    kind: BridgeKind,
    status: StatusCode,
    body: ProviderBody,
    auto_mode_traces: bool,
) -> TestSystem {
    let provider_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let provider_address = provider_listener.local_addr().expect("provider address");
    let state = ProviderState {
        status,
        body,
        attempts: Arc::new(AtomicUsize::new(0)),
        consumed: Arc::new(AtomicUsize::new(0)),
        dropped: Arc::new(AtomicBool::new(false)),
    };
    let provider = Router::new()
        .route("/v1/chat/completions", post(provider_response))
        .with_state(state.clone());
    let provider_task = tokio::spawn(async move {
        axum::serve(provider_listener, provider)
            .await
            .expect("provider should serve");
    });
    let bridge_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge listener");
    let provider_base_url = format!("http://{provider_address}/v1");
    let bridge = spawn_bridge(kind, bridge_listener, provider_base_url, auto_mode_traces);
    TestSystem {
        bridge,
        provider_task,
        state,
        kind,
    }
}

fn spawn_bridge(
    kind: BridgeKind,
    listener: TcpListener,
    provider_base_url: String,
    auto_mode_traces: bool,
) -> RunningBridge {
    match kind {
        BridgeKind::Anthropic => nan_harness_bridge::spawn(
            listener,
            BridgeConfig {
                launch_id: "final-error-anthropic".to_owned(),
                provider_base_url,
                models: ClaudeModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
                    .expect("Claude model catalog"),
                provider_api_key: secret("provider-key"),
                session_token: secret(SESSION_TOKEN),
                web_search_enabled: false,
                auto_mode_traces,
            },
        )
        .expect("Anthropic bridge"),
        BridgeKind::Responses => nan_harness_bridge::spawn_responses(
            listener,
            ResponsesBridgeConfig {
                launch_id: "final-error-responses".to_owned(),
                provider_base_url,
                models: CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
                    .expect("Codex model catalog"),
                provider_api_key: secret("provider-key"),
                session_token: secret(SESSION_TOKEN),
                web_search_enabled: false,
            },
        )
        .expect("Responses bridge"),
        BridgeKind::FxGateway => nan_harness_bridge::spawn_fx_gateway(
            listener,
            FxGatewayConfig {
                launch_id: "final-error-fx".to_owned(),
                provider_base_url,
                models: FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
                    .expect("fx model catalog"),
                selected_model_id: "qwen3.6".to_owned(),
                provider_api_key: secret("provider-key"),
                session_token: secret(SESSION_TOKEN),
                web_search_enabled: false,
            },
        )
        .expect("fx bridge"),
    }
}

async fn provider_response(State(state): State<ProviderState>) -> Response {
    state.attempts.fetch_add(1, Ordering::SeqCst);
    let body = match &state.body {
        ProviderBody::Complete(payload) => Body::from(payload.to_vec()),
        ProviderBody::Chunked(payload) => chunked_body(payload.clone(), &state),
        ProviderBody::Progressing => progressing_body(&state),
    };
    Response::builder()
        .status(state.status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .expect("provider response")
}

fn chunked_body(payload: Arc<[u8]>, state: &ProviderState) -> Body {
    let consumed = Arc::clone(&state.consumed);
    let signal = DropSignal(Arc::clone(&state.dropped));
    Body::from_stream(async_stream::stream! {
        let _signal = signal;
        for chunk in payload.chunks(4 * 1024) {
            consumed.fetch_add(chunk.len(), Ordering::SeqCst);
            yield Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk));
        }
    })
}

fn progressing_body(state: &ProviderState) -> Body {
    let consumed = Arc::clone(&state.consumed);
    let signal = DropSignal(Arc::clone(&state.dropped));
    Body::from_stream(async_stream::stream! {
        let _signal = signal;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            consumed.fetch_add(1, Ordering::SeqCst);
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b"progress-private-marker"));
        }
    })
}

fn padded_error(message: &str, size: usize) -> Vec<u8> {
    let mut body = json!({"error": {"message": message}})
        .to_string()
        .into_bytes();
    assert!(body.len() <= size);
    body.resize(size, b' ');
    body
}

fn provider_error(message: &str) -> Arc<[u8]> {
    json!({"error": {"message": message}})
        .to_string()
        .into_bytes()
        .into()
}

fn responses_request() -> Value {
    json!({
        "model": "qwen3.6",
        "instructions": "Fail safely.",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "fail safely"}]
        }],
        "tools": [],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "store": false,
        "stream": true,
        "include": []
    })
}

fn secret(value: &str) -> Arc<SecretValue> {
    Arc::new(SecretValue::new(value).expect("synthetic secret"))
}
