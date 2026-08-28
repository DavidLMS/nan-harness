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
use axum::http::{HeaderMap, StatusCode, Uri, header};
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

const MICRO_WARMUPS: usize = 100;
const MICRO_SAMPLES: usize = 1_000;
const REALISTIC_WARMUPS: usize = 10;
const REALISTIC_SAMPLES: usize = 100;
const SEQUENTIAL_SAMPLES: usize = 1_000;
const REALISTIC_INITIAL_DELAY: Duration = Duration::from_millis(2);
const REALISTIC_EVENT_DELAY: Duration = Duration::from_millis(1);
const SESSION_TOKEN: &str = "benchmark-session-token";
const PROVIDER_KEY: &str = "benchmark-provider-key";

#[derive(Debug, Serialize)]
struct Report {
    metadata: Metadata,
    profiles: Vec<ProfileMetadata>,
    scenarios: Vec<ScenarioResult>,
    spawn_shutdown: TimingSummary,
    retained_memory: MemoryResult,
    binary: BinaryResult,
}

#[derive(Debug, Serialize)]
struct Metadata {
    mode: &'static str,
    host: String,
    os: String,
    arch: String,
    rustc: String,
    command: String,
}

#[derive(Debug, Serialize)]
struct ProfileMetadata {
    name: &'static str,
    initial_delay_ms: u64,
    event_delay_ms: u64,
    warmups: usize,
    samples: usize,
    sequential_samples: usize,
    note: &'static str,
}

#[derive(Debug, Serialize)]
struct ScenarioResult {
    profile: &'static str,
    name: String,
    payload_bytes: usize,
    concurrency: usize,
    warmups: usize,
    samples: usize,
    wall_clock_ms: f64,
    baseline: RouteSummary,
    gateway: RouteSummary,
    paired_ttft_delta_ms: TimingSummary,
    paired_completion_delta_ms: TimingSummary,
    baseline_wall_clock_throughput_bytes_per_sec: f64,
    gateway_wall_clock_throughput_bytes_per_sec: f64,
    wall_clock_throughput_degradation_percent: f64,
    summed_request_throughput_degradation_percent: f64,
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

#[derive(Debug, Clone, Copy)]
struct ScenarioSpec<'a> {
    profile: BenchmarkProfile,
    name: &'a str,
    payload_bytes: usize,
    stream: bool,
    concurrency: usize,
    samples: usize,
}

#[derive(Debug, Clone, Copy)]
struct BenchmarkProfile {
    name: &'static str,
    initial_delay: Duration,
    event_delay: Duration,
    warmups: usize,
    samples: usize,
    sequential_samples: usize,
    note: &'static str,
}

const MICRO_PROFILE: BenchmarkProfile = BenchmarkProfile {
    name: "micro-zero-delay",
    initial_delay: Duration::ZERO,
    event_delay: Duration::ZERO,
    warmups: MICRO_WARMUPS,
    samples: MICRO_SAMPLES,
    sequential_samples: SEQUENTIAL_SAMPLES,
    note: "Descriptive loopback microbenchmark; not a production latency gate.",
};

const REALISTIC_PROFILE: BenchmarkProfile = BenchmarkProfile {
    name: "realistic-fixed-cadence",
    initial_delay: REALISTIC_INITIAL_DELAY,
    event_delay: REALISTIC_EVENT_DELAY,
    warmups: REALISTIC_WARMUPS,
    samples: REALISTIC_SAMPLES,
    sequential_samples: SEQUENTIAL_SAMPLES,
    note: "Synthetic provider profile with a fixed initial delay and SSE cadence.",
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "experiments/transparent-chat-gateway-v2/results.json".to_owned());
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
    let mut before_rss = None;
    let mut after_rss = None;
    for profile in [MICRO_PROFILE, REALISTIC_PROFILE] {
        for (name, payload_bytes, stream) in [
            ("json-4k", 4 * 1024, false),
            ("json-256k", 256 * 1024, false),
            ("json-4m", 4 * 1024 * 1024, false),
            ("sse-100-events", 4 * 1024, true),
        ] {
            if !selected_scenario(only.as_deref(), profile.name, name) {
                continue;
            }
            eprintln!("running {}/{}", profile.name, name);
            scenarios.push(
                run_scenario(
                    &client,
                    &upstream_url,
                    &gateway_url,
                    ScenarioSpec {
                        profile,
                        name,
                        payload_bytes,
                        stream,
                        concurrency: 1,
                        samples: profile.samples,
                    },
                )
                .await?,
            );
        }

        for concurrency in [1, 8, 32] {
            let name = format!("sse-100-events-concurrency-{concurrency}");
            if !selected_scenario(only.as_deref(), profile.name, &name) {
                continue;
            }
            eprintln!("running {}/{}", profile.name, name);
            scenarios.push(
                run_scenario(
                    &client,
                    &upstream_url,
                    &gateway_url,
                    ScenarioSpec {
                        profile,
                        name: &name,
                        payload_bytes: 4 * 1024,
                        stream: true,
                        concurrency,
                        samples: profile.samples,
                    },
                )
                .await?,
            );
        }

        let name = "sequential-1000-streams";
        if selected_scenario(only.as_deref(), profile.name, name) {
            eprintln!("running {}/{}", profile.name, name);
            if before_rss.is_none() {
                before_rss = process_rss_bytes();
            }
            let sequential = run_scenario(
                &client,
                &upstream_url,
                &gateway_url,
                ScenarioSpec {
                    profile,
                    name,
                    payload_bytes: 4 * 1024,
                    stream: true,
                    concurrency: 1,
                    samples: profile.sequential_samples,
                },
            )
            .await?;
            after_rss = process_rss_bytes();
            scenarios.push(sequential);
        }
    }

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
            host: format!("{}-{}", env::consts::OS, env::consts::ARCH),
            os: env::consts::OS.to_owned(),
            arch: env::consts::ARCH.to_owned(),
            rustc: rustc_version(),
            command: "cargo run --release -p nan-harness-bridge --example chat_gateway_benchmark -- <output> [scenario]".to_owned(),
        },
        profiles: [MICRO_PROFILE, REALISTIC_PROFILE]
            .into_iter()
            .map(profile_metadata)
            .collect(),
        scenarios,
        spawn_shutdown,
        retained_memory: MemoryResult {
            before_rss_bytes: before_rss,
            after_rss_bytes: after_rss,
            delta_bytes: before_rss
                .zip(after_rss)
                .map(|(before, after)| after as i64 - before as i64),
            after_cpu_percent: process_cpu_percent(),
            samples: SEQUENTIAL_SAMPLES,
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
    spec: ScenarioSpec<'_>,
) -> Result<ScenarioResult, Box<dyn std::error::Error>> {
    let ScenarioSpec {
        profile,
        name,
        payload_bytes,
        stream,
        concurrency,
        samples,
    } = spec;
    let body = request_body(payload_bytes, stream);
    let baseline_url = profile_url(baseline_url, profile.name);
    let gateway_url = profile_url(gateway_url, profile.name);
    let mut baseline = Vec::with_capacity(samples);
    let mut gateway = Vec::with_capacity(samples);
    let mut baseline_wall_clock = Duration::ZERO;
    let mut gateway_wall_clock = Duration::ZERO;
    for index in 0..profile.warmups {
        if index % 2 == 0 {
            let _ = measure_request(client, &baseline_url, &body, None).await?;
            let _ = measure_request(client, &gateway_url, &body, Some(SESSION_TOKEN)).await?;
        } else {
            let _ = measure_request(client, &gateway_url, &body, Some(SESSION_TOKEN)).await?;
            let _ = measure_request(client, &baseline_url, &body, None).await?;
        }
    }
    let sample_started = Instant::now();
    if concurrency == 1 {
        for index in 0..samples {
            if index % 2 == 0 {
                let started = Instant::now();
                baseline.push(measure_request(client, &baseline_url, &body, None).await?);
                baseline_wall_clock += started.elapsed();
                let started = Instant::now();
                gateway
                    .push(measure_request(client, &gateway_url, &body, Some(SESSION_TOKEN)).await?);
                gateway_wall_clock += started.elapsed();
            } else {
                let started = Instant::now();
                gateway
                    .push(measure_request(client, &gateway_url, &body, Some(SESSION_TOKEN)).await?);
                gateway_wall_clock += started.elapsed();
                let started = Instant::now();
                baseline.push(measure_request(client, &baseline_url, &body, None).await?);
                baseline_wall_clock += started.elapsed();
            }
        }
    } else {
        let mut completed = 0;
        while completed < samples {
            let batch_size = concurrency.min(samples - completed);
            let gateway_first = (completed / batch_size) % 2 == 1;
            let first = if gateway_first {
                &gateway_url
            } else {
                &baseline_url
            };
            let second = if gateway_first {
                &baseline_url
            } else {
                &gateway_url
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
            let first_started = Instant::now();
            let first_results = futures_util::future::try_join_all(first_tasks).await?;
            let first_wall_clock = first_started.elapsed();
            let second_started = Instant::now();
            let second_results = futures_util::future::try_join_all(second_tasks).await?;
            let second_wall_clock = second_started.elapsed();
            if gateway_first {
                gateway.extend(first_results);
                baseline.extend(second_results);
                gateway_wall_clock += first_wall_clock;
                baseline_wall_clock += second_wall_clock;
            } else {
                baseline.extend(first_results);
                gateway.extend(second_results);
                baseline_wall_clock += first_wall_clock;
                gateway_wall_clock += second_wall_clock;
            }
            completed += batch_size;
        }
    }
    let wall_clock = sample_started.elapsed();
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
    let baseline_bytes = baseline
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>();
    let gateway_bytes = gateway
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>();
    let baseline_wall_clock_throughput =
        baseline_bytes / baseline_wall_clock.as_secs_f64().max(f64::EPSILON);
    let gateway_wall_clock_throughput =
        gateway_bytes / gateway_wall_clock.as_secs_f64().max(f64::EPSILON);
    Ok(ScenarioResult {
        profile: profile.name,
        name: name.to_owned(),
        payload_bytes,
        concurrency,
        warmups: profile.warmups,
        samples,
        wall_clock_ms: wall_clock.as_secs_f64() * 1_000.0,
        paired_ttft_delta_ms: summarize_deltas(&baseline, &gateway, |timing| timing.first_byte),
        paired_completion_delta_ms: summarize_deltas(&baseline, &gateway, |timing| {
            timing.completion
        }),
        baseline_wall_clock_throughput_bytes_per_sec: baseline_wall_clock_throughput,
        gateway_wall_clock_throughput_bytes_per_sec: gateway_wall_clock_throughput,
        wall_clock_throughput_degradation_percent: (1.0
            - gateway_wall_clock_throughput / baseline_wall_clock_throughput)
            * 100.0,
        summed_request_throughput_degradation_percent: (1.0
            - gateway_throughput / baseline_throughput)
            * 100.0,
        baseline: route_summary(&baseline),
        gateway: route_summary(&gateway),
    })
}

fn selected_scenario(only: Option<&str>, profile: &str, name: &str) -> bool {
    let Some(selected) = only else {
        return true;
    };
    selected == name || selected == format!("{profile}/{name}")
}

fn profile_url(endpoint: &str, profile: &str) -> String {
    format!("{endpoint}?profile={profile}")
}

fn profile_metadata(profile: BenchmarkProfile) -> ProfileMetadata {
    ProfileMetadata {
        name: profile.name,
        initial_delay_ms: profile.initial_delay.as_millis() as u64,
        event_delay_ms: profile.event_delay.as_millis() as u64,
        warmups: profile.warmups,
        samples: profile.samples,
        sequential_samples: profile.sequential_samples,
        note: profile.note,
    }
}

fn route_summary(values: &[Timing]) -> RouteSummary {
    let headers = values
        .iter()
        .map(|timing| timing.headers)
        .collect::<Vec<_>>();
    let time_to_first_byte = values
        .iter()
        .map(|timing| timing.first_byte)
        .collect::<Vec<_>>();
    let completion = values
        .iter()
        .map(|timing| timing.completion)
        .collect::<Vec<_>>();
    RouteSummary {
        headers: summarize(&headers),
        time_to_first_byte: summarize(&time_to_first_byte),
        completion: summarize(&completion),
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

fn summarize(values: &[Duration]) -> TimingSummary {
    let millis = values
        .iter()
        .map(Duration::as_secs_f64)
        .map(|value| value * 1_000.0)
        .collect::<Vec<_>>();
    summarize_millis(millis)
}

fn summarize_millis(mut millis: Vec<f64>) -> TimingSummary {
    millis.sort_unstable_by(f64::total_cmp);
    let samples = millis.len();
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
    summarize_millis(
        baseline
            .iter()
            .zip(gateway)
            .map(|(baseline, gateway)| {
                selector(gateway).as_secs_f64() * 1_000.0
                    - selector(baseline).as_secs_f64() * 1_000.0
            })
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
    Ok(summarize(&values))
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

fn profile_from_query(query: Option<&str>) -> BenchmarkProfile {
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
