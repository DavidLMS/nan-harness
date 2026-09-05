use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};
use std::time::Duration;

use super::super::{REALISTIC_EVENT_DELAY, REALISTIC_INITIAL_DELAY};

#[derive(Debug, Clone, Copy)]
pub(crate) struct BenchmarkProfile {
    pub(crate) name: &'static str,
    pub(crate) initial_delay: Duration,
    pub(crate) event_delay: Duration,
    pub(crate) warmups: usize,
    pub(crate) samples: usize,
    pub(crate) sequential_samples: usize,
    pub(crate) note: &'static str,
}

pub(crate) const MICRO_PROFILE: BenchmarkProfile = BenchmarkProfile {
    name: "micro-zero-delay",
    initial_delay: Duration::ZERO,
    event_delay: Duration::ZERO,
    warmups: super::super::MICRO_WARMUPS,
    samples: super::super::MICRO_SAMPLES,
    sequential_samples: super::super::SEQUENTIAL_SAMPLES,
    note: "Descriptive loopback microbenchmark; not a production latency gate.",
};

pub(crate) const REALISTIC_PROFILE: BenchmarkProfile = BenchmarkProfile {
    name: "realistic-fixed-cadence",
    initial_delay: REALISTIC_INITIAL_DELAY,
    event_delay: REALISTIC_EVENT_DELAY,
    warmups: super::super::REALISTIC_WARMUPS,
    samples: super::super::REALISTIC_SAMPLES,
    sequential_samples: super::super::SEQUENTIAL_SAMPLES,
    note: "Synthetic provider profile with a fixed initial delay and SSE cadence.",
};

pub(crate) fn router() -> Router {
    Router::new()
        .route("/v1/chat/completions", post(fake_chat))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
}

pub(crate) fn profile_url(endpoint: &str, profile: BenchmarkProfile) -> String {
    format!("{endpoint}?profile={}", profile.name)
}

pub(crate) fn profile_from_query(query: Option<&str>) -> BenchmarkProfile {
    if query
        .unwrap_or_default()
        .split('&')
        .any(|part| part == "profile=realistic-fixed-cadence")
    {
        REALISTIC_PROFILE
    } else {
        MICRO_PROFILE
    }
}

pub(crate) fn request_body(payload_bytes: usize, stream: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "model": "qwen3.6",
        "messages": [{"role":"user","content":"benchmark"}],
        "stream": stream,
        "payload": "x".repeat(payload_bytes)
    }))
    .expect("benchmark body should serialize")
}

async fn fake_chat(uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value != "Bearer benchmark-provider-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request: Value = serde_json::from_slice(&body).expect("benchmark request should be JSON");
    let profile = profile_from_query(uri.query());
    if !profile.initial_delay.is_zero() {
        tokio::time::sleep(profile.initial_delay).await;
    }
    if request["stream"] == true {
        let events = (0..100).map(|index| {
            format!("data: {{\"id\":\"{index}\",\"choices\":[{{\"delta\":{{\"content\":\"x\"}}}}]}}\n\n")
        }).collect::<Vec<_>>();
        let body = async_stream::stream! {
            for (index, event) in events.into_iter().enumerate() {
                if index > 0 && !profile.event_delay.is_zero() {
                    tokio::time::sleep(profile.event_delay).await;
                }
                yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(event));
            }
            if !profile.event_delay.is_zero() {
                tokio::time::sleep(profile.event_delay).await;
            }
            yield Ok::<Bytes, std::convert::Infallible>(Bytes::from_static(
                b"data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            ));
            yield Ok::<Bytes, std::convert::Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
        };
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(body))
            .expect("benchmark stream response");
    }
    axum::Json(json!({
        "id":"benchmark",
        "choices":[{"message":{"content":"ok"}}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{MICRO_PROFILE, REALISTIC_PROFILE, profile_from_query, profile_url};

    #[test]
    fn profile_query_selects_realistic_only_for_exact_profile_pair() {
        assert_eq!(
            profile_from_query(Some("profile=realistic-fixed-cadence")).name,
            REALISTIC_PROFILE.name
        );
        assert_eq!(
            profile_from_query(Some("x=1&profile=realistic-fixed-cadence")).name,
            REALISTIC_PROFILE.name
        );
        assert_eq!(
            profile_from_query(Some("profile=micro-zero-delay")).name,
            MICRO_PROFILE.name
        );
        assert_eq!(
            profile_from_query(Some("profile=realistic-fixed-cadence-extra")).name,
            MICRO_PROFILE.name
        );
        assert_eq!(
            profile_url("http://localhost", MICRO_PROFILE),
            "http://localhost?profile=micro-zero-delay"
        );
    }
}
