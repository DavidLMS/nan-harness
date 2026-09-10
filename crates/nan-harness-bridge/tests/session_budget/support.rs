use axum::{
    Json, Router,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::post,
};
use nan_harness_bridge::{
    BridgeConfig, ChatCompletionsBridgeConfig, ClaudeModelCatalog, CodexModelCatalog,
    FxGatewayConfig, FxModelCatalog, ResponsesBridgeConfig, RunningBridge,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::net::TcpListener;

pub const TOKEN: &str = "synthetic-session";
pub const MODEL: &str = "qwen3.6";

#[derive(Debug, Clone, Copy)]
pub enum Transport {
    Responses,
    Anthropic,
    Chat,
    Fx,
}

pub const TRANSPORTS: [Transport; 4] = [
    Transport::Responses,
    Transport::Anthropic,
    Transport::Chat,
    Transport::Fx,
];

impl Transport {
    pub fn path(self) -> &'static str {
        match self {
            Self::Responses => "/v1/responses",
            Self::Anthropic => "/v1/messages",
            Self::Chat => "/v1/chat/completions",
            Self::Fx => "/v3/ai/language-model",
        }
    }

    pub fn request(self, streaming: bool) -> Value {
        match self {
            Self::Responses => {
                json!({"model":MODEL,"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"Synthetic test"}]}],"stream":true})
            }
            Self::Anthropic => {
                json!({"model":format!("anthropic/nan/{MODEL}"),"messages":[{"role":"user","content":"Synthetic test"}],"max_tokens":128,"stream":streaming})
            }
            Self::Chat => {
                json!({"model":MODEL,"messages":[{"role":"user","content":"Synthetic test"}],"stream":streaming})
            }
            Self::Fx => {
                json!({"prompt":[{"role":"user","content":[{"type":"text","text":"Synthetic test"}]}]})
            }
        }
    }

    pub fn constrained(self) -> Vec<Value> {
        let mut format = self.request(true);
        let mut required = self.request(true);
        match self {
            Self::Responses => {
                format["text"] = json!({"format":{"type":"json_schema","name":"test","schema":{"type":"object"}}});
                required["tool_choice"] = json!("required");
            }
            Self::Anthropic => {
                format["output_config"] =
                    json!({"format":{"type":"json_schema","schema":{"type":"object"}}});
                required["tool_choice"] = json!({"type":"any"});
            }
            Self::Chat => {
                format["response_format"] = json!({"type":"json_object"});
                required["tool_choice"] = json!("required");
            }
            Self::Fx => {
                format["responseFormat"] = json!({"type":"json"});
                required["toolChoice"] = json!({"type":"required"});
            }
        }
        let mut contracts = vec![format, required];
        match self {
            Self::Anthropic => {
                let mut permission = self.request(false);
                permission["max_tokens"] = json!(64);
                permission["system"] = json!(
                    "You are a security monitor for autonomous AI coding agents.\n## Classification Process\n## Output Format"
                );
                permission["messages"] = json!([{"role":"user","content":"Stage 1 does NOT apply user intent or ALLOW exceptions"}]);
                contracts.push(permission);
            }
            Self::Fx => {
                let mut permission = self.request(true);
                permission["tools"] = json!([{"type":"function","name":"permission_decision","inputSchema":{"type":"object"}}]);
                contracts.push(permission);
            }
            Self::Responses | Self::Chat => {}
        }
        contracts
    }
}

#[derive(Clone, Default)]
pub struct Provider {
    pub sends: Arc<AtomicUsize>,
    pub empty: Arc<AtomicBool>,
    pub hold: Arc<AtomicBool>,
    pub released: Arc<tokio::sync::Notify>,
}

pub struct System {
    pub bridge: RunningBridge,
    pub provider: Provider,
    pub transport: Transport,
    task: tokio::task::JoinHandle<()>,
}

impl System {
    pub async fn send(&self, body: &Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!(
                "{}{}",
                self.bridge.base_url(),
                self.transport.path()
            ))
            .bearer_auth(TOKEN)
            .header("ai-language-model-id", MODEL)
            .json(body)
            .send()
            .await
            .expect("bridge request")
    }

    pub async fn stop(mut self) {
        self.bridge.shutdown();
        self.bridge.wait().await.expect("clean shutdown");
        self.task.abort();
    }
}

pub async fn start(transport: Transport, limit: u64) -> System {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let address = listener.local_addr().expect("address");
    let provider = Provider::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(answer))
        .with_state(provider.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("provider server");
    });
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge listener");
    let provider_base_url = format!("http://{address}/v1");
    let launch_id = format!("budget-{transport:?}-{address}");
    let provider_api_key = secret("synthetic-provider");
    let session_token = secret(TOKEN);
    let session_max_tokens = Some(limit);
    let bridge = match transport {
        Transport::Responses => nan_harness_bridge::spawn_responses(
            listener,
            ResponsesBridgeConfig {
                launch_id,
                provider_base_url,
                models: CodexModelCatalog::from_provider_ids([MODEL.to_owned()], MODEL)
                    .expect("models"),
                provider_api_key,
                session_token,
                web_search_enabled: false,
                session_max_tokens,
            },
        ),
        Transport::Anthropic => nan_harness_bridge::spawn(
            listener,
            BridgeConfig {
                launch_id,
                provider_base_url,
                models: ClaudeModelCatalog::from_provider_ids([MODEL.to_owned()], MODEL)
                    .expect("models"),
                provider_api_key,
                session_token,
                web_search_enabled: false,
                auto_mode_traces: false,
                session_max_tokens,
            },
        ),
        Transport::Chat => nan_harness_bridge::spawn_chat_completions(
            listener,
            ChatCompletionsBridgeConfig {
                launch_id,
                provider_base_url,
                model_id: MODEL.to_owned(),
                provider_api_key,
                session_token,
                web_search_enabled: false,
                session_max_tokens,
            },
        ),
        Transport::Fx => nan_harness_bridge::spawn_fx_gateway(
            listener,
            FxGatewayConfig {
                launch_id,
                provider_base_url,
                models: FxModelCatalog::from_provider_ids([MODEL.to_owned()]).expect("models"),
                selected_model_id: MODEL.to_owned(),
                provider_api_key,
                session_token,
                web_search_enabled: false,
                session_max_tokens,
            },
        ),
    }
    .expect("bridge server");
    System {
        bridge,
        provider,
        transport,
        task,
    }
}

fn secret(value: &str) -> Arc<SecretValue> {
    Arc::new(SecretValue::new(value).expect("synthetic secret"))
}

async fn answer(State(state): State<Provider>, Json(body): Json<Value>) -> Response {
    state.sends.fetch_add(1, Ordering::SeqCst);
    loop {
        let released = state.released.notified();
        if !state.hold.load(Ordering::SeqCst) {
            break;
        }
        released.await;
    }
    let text = if state.empty.load(Ordering::SeqCst) {
        ""
    } else {
        "Synthetic response"
    };
    let usage = json!({"prompt_tokens":20,"completion_tokens":10,"total_tokens":30});
    if body["stream"] == true {
        let content = json!({"id":"synthetic","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
        let finish = json!({"id":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":usage});
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            format!("data: {content}\n\ndata: {finish}\n\ndata: [DONE]\n\n"),
        )
            .into_response()
    } else {
        Json(json!({"id":"synthetic","object":"chat.completion","model":MODEL,"choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":usage})).into_response()
    }
}

pub fn events(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|line| *line != "[DONE]")
        .map(|line| serde_json::from_str(line).expect("valid SSE JSON"))
        .collect()
}
