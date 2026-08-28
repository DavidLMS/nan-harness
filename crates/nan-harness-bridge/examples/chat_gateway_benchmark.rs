#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures_util::StreamExt;
use nan_harness_bridge::{ChatCompletionsBridgeConfig, spawn_chat_completions};
use nan_harness_core::SecretValue;
use serde::Serialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const WARMUPS: usize = 100;
const SAMPLES: usize = 1_000;
const SESSION_TOKEN: &str = "benchmark-session-token";
const PROVIDER_KEY: &str = "benchmark-provider-key";

#[derive(Debug, Serialize)]
struct Report {
    metadata: Metadata,
    scenarios: Vec<ScenarioResult>,
    spawn_shutdown: TimingSummary,
    retained_memory: MemoryResult,
    binary: BinaryResult,
}

#[derive(Debug, Serialize)]
struct Metadata {
    mode: &'static str,
    warmups: usize,
    samples: usize,
    os: String,
    arch: String,
    rustc: String,
    command: String,
}

#[derive(Debug, Serialize)]
struct ScenarioResult {
    name: String,
    payload_bytes: usize,
    concurrency: usize,
    baseline: RouteSummary,
    gateway: RouteSummary,
    added_ttft_ms: TimingSummary,
    added_completion_ms: TimingSummary,
    throughput_degradation_percent: f64,
}

#[derive(Debug, Serialize)]
struct RouteSummary {
    headers: TimingSummary,
    time_to_first_byte: TimingSummary,
    completion: TimingSummary,
    response_bytes: usize,
}

#[derive(Debug, Serialize, Clone)]
struct TimingSummary {
    samples: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    mean_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

#[derive(Debug, Serialize)]
struct MemoryResult {
    before_rss_bytes: Option<u64>,
    after_rss_bytes: Option<u64>,
    delta_bytes: Option<i64>,
    after_cpu_percent: Option<f64>,
    samples: usize,
    note: &'static str,
}

#[derive(Debug, Serialize)]
struct BinaryResult {
    current_executable_bytes: Option<u64>,
    release_cli_bytes: Option<u64>,
    baseline_cli_bytes: Option<u64>,
    note: &'static str,
}

#[derive(Debug, Clone)]
struct Timing {
    headers: Duration,
    first_byte: Duration,
    completion: Duration,
    body_bytes: usize,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "experiments/transparent-chat-gateway/results.json".to_owned());
    let only = env::args().nth(2);
    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }

    let upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
    let upstream_address = upstream_listener.local_addr()?;
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            upstream_listener,
            Router::new()
                .route("/v1/chat/completions", post(fake_chat))
                .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .await
        .expect("synthetic upstream should serve");
    });
    let upstream_url = format!("http://{upstream_address}/v1/chat/completions");

    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(64)
        .build()?;
    let bridge_listener = TcpListener::bind("127.0.0.1:0").await?;
    let bridge = spawn_chat_completions(
        bridge_listener,
        ChatCompletionsBridgeConfig {
            provider_base_url: format!("http://{upstream_address}/v1"),
            provider_api_key: Arc::new(SecretValue::new(PROVIDER_KEY)?),
            session_token: Arc::new(SecretValue::new(SESSION_TOKEN)?),
        },
    )?;
    let gateway_url = format!("{}/v1/chat/completions", bridge.base_url());

    let mut scenarios = Vec::new();
    for (name, payload_bytes, stream) in [
        ("json-4k", 4 * 1024, false),
        ("json-256k", 256 * 1024, false),
        ("json-4m", 4 * 1024 * 1024, false),
        ("sse-100-events", 4 * 1024, true),
    ] {
        if only.as_deref().is_some_and(|selected| selected != name) {
            continue;
        }
        eprintln!("running {name}");
        scenarios.push(
            run_scenario(
                &client,
                &upstream_url,
                &gateway_url,
                name,
                payload_bytes,
                stream,
                1,
            )
            .await?,
        );
    }

    for concurrency in [1, 8, 32] {
        let name = format!("sse-100-events-concurrency-{concurrency}");
        if only.as_deref().is_some_and(|selected| selected != name) {
            continue;
        }
        eprintln!("running {name}");
        scenarios.push(
            run_scenario(
                &client,
                &upstream_url,
                &gateway_url,
                &name,
                4 * 1024,
                true,
                concurrency,
            )
            .await?,
        );
    }

    let (before_rss, after_rss) = if only
        .as_deref()
        .is_none_or(|selected| selected == "sequential-1000-streams")
    {
        eprintln!("running sequential-1000-streams");
        let before_rss = process_rss_bytes();
        let sequential = run_scenario(
            &client,
            &upstream_url,
            &gateway_url,
            "sequential-1000-streams",
            4 * 1024,
            true,
            1,
        )
        .await?;
        let after_rss = process_rss_bytes();
        scenarios.push(sequential);
        (before_rss, after_rss)
    } else {
        (None, None)
    };

    let spawn_shutdown = if only
        .as_deref()
        .is_none_or(|selected| selected == "spawn-shutdown")
    {
        eprintln!("running spawn-shutdown");
        measure_spawn_shutdown(&format!("http://{upstream_address}/v1")).await?
    } else {
        TimingSummary {
            samples: 0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            mean_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
        }
    };

    let mut bridge = bridge;
    bridge.shutdown();
    bridge.wait().await?;
    upstream_task.abort();

    let report = Report {
        metadata: Metadata {
            mode: "release synthetic A/B",
            warmups: WARMUPS,
            samples: SAMPLES,
            os: env::consts::OS.to_owned(),
            arch: env::consts::ARCH.to_owned(),
            rustc: rustc_version(),
            command: "cargo run --release -p nan-harness-bridge --example chat_gateway_benchmark -- <output> [scenario]".to_owned(),
        },
        scenarios,
        spawn_shutdown,
        retained_memory: MemoryResult {
            before_rss_bytes: before_rss,
            after_rss_bytes: after_rss,
            delta_bytes: before_rss
                .zip(after_rss)
                .map(|(before, after)| after as i64 - before as i64),
            after_cpu_percent: process_cpu_percent(),
            samples: SAMPLES,
            note: "RSS and CPU are optional and use ps when available; run twice and compare p95 manually.",
        },
        binary: BinaryResult {
            current_executable_bytes: env::current_exe()
                .ok()
                .and_then(|path| fs::metadata(path).ok().map(|meta| meta.len())),
            release_cli_bytes: fs::metadata("target/release/nan-harness")
                .ok()
                .map(|meta| meta.len()),
            baseline_cli_bytes: None,
            note: "No pre-spike baseline binary was present in target/release; size gate needs an external baseline.",
        },
    };
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("wrote {output}");
    Ok(())
}

async fn run_scenario(
    client: &reqwest::Client,
    baseline_url: &str,
    gateway_url: &str,
    name: &str,
    payload_bytes: usize,
    stream: bool,
    concurrency: usize,
) -> Result<ScenarioResult, Box<dyn std::error::Error>> {
    let body = request_body(payload_bytes, stream);
    let mut baseline = Vec::with_capacity(SAMPLES);
    let mut gateway = Vec::with_capacity(SAMPLES);
    for index in 0..WARMUPS {
        if index % 2 == 0 {
            let _ = measure_request(client, baseline_url, &body, None).await?;
            let _ = measure_request(client, gateway_url, &body, Some(SESSION_TOKEN)).await?;
        } else {
            let _ = measure_request(client, gateway_url, &body, Some(SESSION_TOKEN)).await?;
            let _ = measure_request(client, baseline_url, &body, None).await?;
        }
    }
    if concurrency == 1 {
        for index in 0..SAMPLES {
            if index % 2 == 0 {
                baseline.push(measure_request(client, baseline_url, &body, None).await?);
                gateway
                    .push(measure_request(client, gateway_url, &body, Some(SESSION_TOKEN)).await?);
            } else {
                gateway
                    .push(measure_request(client, gateway_url, &body, Some(SESSION_TOKEN)).await?);
                baseline.push(measure_request(client, baseline_url, &body, None).await?);
            }
        }
    } else {
        let mut completed = 0;
        while completed < SAMPLES {
            let batch_size = concurrency.min(SAMPLES - completed);
            let gateway_first = (completed / batch_size) % 2 == 1;
            let first = if gateway_first {
                gateway_url
            } else {
                baseline_url
            };
            let second = if gateway_first {
                baseline_url
            } else {
                gateway_url
            };
            let first_token = gateway_first.then_some(SESSION_TOKEN);
            let second_token = (!gateway_first).then_some(SESSION_TOKEN);
            let mut first_tasks = Vec::with_capacity(batch_size);
            for _ in 0..batch_size {
                first_tasks.push(measure_request(client, first, &body, first_token));
            }
            let mut second_tasks = Vec::with_capacity(batch_size);
            for _ in 0..batch_size {
                second_tasks.push(measure_request(client, second, &body, second_token));
            }
            let first_results = futures_util::future::try_join_all(first_tasks).await?;
            let second_results = futures_util::future::try_join_all(second_tasks).await?;
            if gateway_first {
                gateway.extend(first_results);
                baseline.extend(second_results);
            } else {
                baseline.extend(first_results);
                gateway.extend(second_results);
            }
            completed += batch_size;
        }
    }

    let baseline_duration = baseline
        .iter()
        .map(|timing| timing.completion)
        .sum::<Duration>();
    let gateway_duration = gateway
        .iter()
        .map(|timing| timing.completion)
        .sum::<Duration>();
    let baseline_throughput = baseline
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>()
        / baseline_duration.as_secs_f64().max(f64::EPSILON);
    let gateway_throughput = gateway
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>()
        / gateway_duration.as_secs_f64().max(f64::EPSILON);
    Ok(ScenarioResult {
        name: name.to_owned(),
        payload_bytes,
        concurrency,
        added_ttft_ms: summarize_deltas(&baseline, &gateway, |timing| timing.first_byte),
        added_completion_ms: summarize_deltas(&baseline, &gateway, |timing| timing.completion),
        throughput_degradation_percent: (1.0 - gateway_throughput / baseline_throughput) * 100.0,
        baseline: route_summary(&baseline),
        gateway: route_summary(&gateway),
    })
}

fn route_summary(values: &[Timing]) -> RouteSummary {
    RouteSummary {
        headers: summarize(values.iter().map(|timing| timing.headers).collect()),
        time_to_first_byte: summarize(values.iter().map(|timing| timing.first_byte).collect()),
        completion: summarize(values.iter().map(|timing| timing.completion).collect()),
        response_bytes: values.iter().map(|timing| timing.body_bytes).sum(),
    }
}

async fn measure_request(
    client: &reqwest::Client,
    endpoint: &str,
    body: &[u8],
    token: Option<&str>,
) -> Result<Timing, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut request = client.post(endpoint).body(body.to_vec());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await?;
    let headers = started.elapsed();
    let mut stream = response.bytes_stream();
    let first_chunk = stream
        .next()
        .await
        .ok_or("response did not contain a body")??;
    let first_byte = started.elapsed();
    let mut body_bytes = first_chunk.len();
    while let Some(chunk) = stream.next().await {
        body_bytes += chunk?.len();
    }
    Ok(Timing {
        headers,
        first_byte,
        completion: started.elapsed(),
        body_bytes,
    })
}

fn request_body(payload_bytes: usize, stream: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "model": "qwen3.6",
        "messages": [{"role":"user","content":"benchmark"}],
        "stream": stream,
        "payload": "x".repeat(payload_bytes)
    }))
    .expect("benchmark body should serialize")
}

fn summarize(mut values: Vec<Duration>) -> TimingSummary {
    values.sort_unstable();
    let samples = values.len();
    let millis = values
        .iter()
        .map(Duration::as_secs_f64)
        .map(|value| value * 1_000.0)
        .collect::<Vec<_>>();
    TimingSummary {
        samples,
        p50_ms: percentile(&millis, 0.50),
        p95_ms: percentile(&millis, 0.95),
        p99_ms: percentile(&millis, 0.99),
        mean_ms: millis.iter().sum::<f64>() / samples.max(1) as f64,
        min_ms: millis.first().copied().unwrap_or_default(),
        max_ms: millis.last().copied().unwrap_or_default(),
    }
}

fn summarize_deltas(
    baseline: &[Timing],
    gateway: &[Timing],
    selector: impl Fn(&Timing) -> Duration,
) -> TimingSummary {
    summarize(
        baseline
            .iter()
            .zip(gateway)
            .map(|(baseline, gateway)| selector(gateway).saturating_sub(selector(baseline)))
            .collect(),
    )
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values[index]
}

async fn measure_spawn_shutdown(
    provider_base_url: &str,
) -> Result<TimingSummary, Box<dyn std::error::Error>> {
    let mut values = Vec::with_capacity(100);
    for _ in 0..100 {
        let started = Instant::now();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut bridge = spawn_chat_completions(
            listener,
            ChatCompletionsBridgeConfig {
                provider_base_url: provider_base_url.to_owned(),
                provider_api_key: Arc::new(SecretValue::new(PROVIDER_KEY)?),
                session_token: Arc::new(SecretValue::new(SESSION_TOKEN)?),
            },
        )?;
        bridge.shutdown();
        bridge.wait().await?;
        values.push(started.elapsed());
    }
    Ok(summarize(values))
}

async fn fake_chat(headers: HeaderMap, body: Bytes) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value != "Bearer benchmark-provider-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request: Value = serde_json::from_slice(&body).expect("benchmark request should be JSON");
    if request["stream"] == true {
        let events = (0..100).map(|index| {
            format!("data: {{\"id\":\"{index}\",\"choices\":[{{\"delta\":{{\"content\":\"x\"}}}}]}}\n\n")
        }).collect::<Vec<_>>();
        let body = async_stream::stream! {
            for event in events {
                yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(event));
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

fn process_rss_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|kilobytes| kilobytes * 1024)
}

fn process_cpu_percent() -> Option<f64> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "%cpu=", "-p", &pid])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(
            || "unavailable".to_owned(),
            |version| version.trim().to_owned(),
        )
}
