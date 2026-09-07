use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use nan_harness_bridge::{
    ChatCompletionsBridgeConfig, CodexModelCatalog, ResponsesBridgeConfig, RunningBridge,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct ProviderState {
    attempts: Arc<AtomicUsize>,
    dropped_bodies: Arc<AtomicUsize>,
}

struct DropSignal(Arc<AtomicUsize>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn responses_bridge_retries_without_reading_an_unfinished_error_body() {
    let (provider_url, state, provider_task) = start_provider().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("responses bridge listener");
    let mut bridge = nan_harness_bridge::spawn_responses(
        listener,
        ResponsesBridgeConfig {
            launch_id: "retry-body-responses".to_owned(),
            provider_base_url: provider_url,
            models: CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
                .expect("model catalog"),
            provider_api_key: secret("provider-key"),
            session_token: secret("session-key"),
            web_search_enabled: false,
        },
    )
    .expect("responses bridge");

    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let response = reqwest::Client::new()
            .post(format!("{}/v1/responses", bridge.base_url()))
            .bearer_auth("session-key")
            .json(&json!({
                "model": "qwen3.6",
                "instructions": "Use tools when needed.",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "retry"}]
                }],
                "tools": [],
                "tool_choice": "auto",
                "parallel_tool_calls": true,
                "store": false,
                "stream": true,
                "include": []
            }))
            .send()
            .await
            .expect("responses request");
        assert_eq!(response.status(), StatusCode::OK);
        response.text().await.expect("responses body")
    })
    .await;
    shutdown(&mut bridge, provider_task).await;

    let body = result.expect("responses retry should reach the second attempt");
    assert!(body.contains("response.completed"), "{body}");
    assert_eq!(state.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(state.dropped_bodies.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chat_proxy_retries_without_reading_an_unfinished_error_body() {
    let (provider_url, state, provider_task) = start_provider().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("chat bridge listener");
    let mut bridge = nan_harness_bridge::spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            launch_id: "retry-body-chat".to_owned(),
            provider_base_url: provider_url,
            model_id: "qwen3.6".to_owned(),
            provider_api_key: secret("provider-key"),
            session_token: secret("session-key"),
            web_search_enabled: false,
        },
    )
    .expect("chat bridge");

    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let response = reqwest::Client::new()
            .post(format!("{}/v1/chat/completions", bridge.base_url()))
            .bearer_auth("session-key")
            .json(&json!({
                "model": "qwen3.6",
                "messages": [{"role": "user", "content": "retry"}],
                "stream": false
            }))
            .send()
            .await
            .expect("chat request");
        assert_eq!(response.status(), StatusCode::OK);
        response.json::<Value>().await.expect("chat body")
    })
    .await;
    shutdown(&mut bridge, provider_task).await;

    let body = result.expect("chat retry should reach the second attempt");
    assert_eq!(body["choices"][0]["message"]["content"], "done");
    assert_eq!(state.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(state.dropped_bodies.load(Ordering::SeqCst), 1);
}

async fn start_provider() -> (String, ProviderState, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let address = listener.local_addr().expect("provider address");
    let state = ProviderState::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(provider_response))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("provider should serve");
    });
    (format!("http://{address}/v1"), state, task)
}

async fn provider_response(
    State(state): State<ProviderState>,
    Json(request): Json<Value>,
) -> Response {
    if state.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
        let signal = DropSignal(Arc::clone(&state.dropped_bodies));
        let body = Body::from_stream(async_stream::stream! {
            let _signal = signal;
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b"still failing"));
            pending::<()>().await;
        });
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(body)
            .expect("retryable provider response");
    }

    let body = if request["stream"] == true {
        Body::from(concat!(
            "data: {\"id\":\"chatcmpl-retry\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"done\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-retry\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        ))
    } else {
        Body::from(
            json!({
                "id": "chatcmpl-retry",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "done"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })
            .to_string(),
        )
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(
            "content-type",
            if request["stream"] == true {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(body)
        .expect("successful provider response")
}

async fn shutdown(bridge: &mut RunningBridge, provider_task: tokio::task::JoinHandle<()>) {
    bridge.shutdown();
    bridge.wait().await.expect("bridge should stop cleanly");
    provider_task.abort();
    let result = provider_task.await;
    assert!(result.is_err_and(|error| error.is_cancelled()));
}

fn secret(value: &str) -> Arc<SecretValue> {
    Arc::new(SecretValue::new(value).expect("synthetic secret"))
}
