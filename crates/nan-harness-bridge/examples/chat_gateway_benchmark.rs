#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

use nan_harness_bridge::{ChatCompletionsBridgeConfig, spawn_chat_completions};
use nan_harness_core::SecretValue;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

#[path = "chat_gateway_benchmark/mod.rs"]
mod benchmark;

use benchmark::provider::{MICRO_PROFILE, REALISTIC_PROFILE, profile_url, request_body, router};
use benchmark::report::{
    BinaryResult, MemoryResult, Metadata, Report, StabilityResult, profile_metadata,
};
use benchmark::scenarios::{RequestTracker, ScenarioSpec, run, selected_scenario};
use benchmark::statistics::{TimingSummary, summarize};

const MICRO_WARMUPS: usize = 100;
const MICRO_SAMPLES: usize = 1_000;
const REALISTIC_WARMUPS: usize = 10;
const REALISTIC_SAMPLES: usize = 100;
const SEQUENTIAL_SAMPLES: usize = 1_000;
const REALISTIC_INITIAL_DELAY: Duration = Duration::from_millis(25);
const REALISTIC_EVENT_DELAY: Duration = Duration::from_millis(1);
const SESSION_TOKEN: &str = "benchmark-session-token";
const PROVIDER_KEY: &str = "benchmark-provider-key";

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
        axum::serve(upstream_listener, router())
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
            launch_id: "benchmark_main".to_owned(),
            provider_base_url: format!("http://{upstream_address}/v1"),
            model_id: "qwen3.6".to_owned(),
            provider_api_key: Arc::new(SecretValue::new(PROVIDER_KEY)?),
            session_token: Arc::new(SecretValue::new(SESSION_TOKEN)?),
            web_search_enabled: true,
            search_config: None,
            session_max_tokens: None,
        },
    )?;
    let gateway_url = format!("{}/v1/chat/completions", bridge.base_url());

    let tracker = RequestTracker::default();
    let open_fds_before = process_open_fd_count();
    let mut scenarios = Vec::new();
    for profile in [MICRO_PROFILE, REALISTIC_PROFILE] {
        for (name, payload_bytes, stream) in [
            ("json-4k", 4 * 1024, false),
            ("json-256k", 256 * 1024, false),
            ("json-4m", 4 * 1024 * 1024, false),
            ("sse-100-events", 4 * 1024, true),
        ] {
            if selected_scenario(only.as_deref(), profile.name, name) {
                eprintln!("running {}/{}", profile.name, name);
                scenarios.push(
                    run(
                        &client,
                        &upstream_url,
                        &gateway_url,
                        &tracker,
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
        }
        for concurrency in [1, 8, 32] {
            let name = format!("sse-100-events-concurrency-{concurrency}");
            if selected_scenario(only.as_deref(), profile.name, &name) {
                eprintln!("running {}/{}", profile.name, name);
                scenarios.push(
                    run(
                        &client,
                        &upstream_url,
                        &gateway_url,
                        &tracker,
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
        }
        let name = "sequential-1000-streams";
        if selected_scenario(only.as_deref(), profile.name, name) {
            eprintln!("running {}/{}", profile.name, name);
            scenarios.push(
                run(
                    &client,
                    &upstream_url,
                    &gateway_url,
                    &tracker,
                    ScenarioSpec {
                        profile,
                        name,
                        payload_bytes: 4 * 1024,
                        stream: true,
                        concurrency: 1,
                        samples: profile.sequential_samples,
                    },
                )
                .await?,
            );
        }
    }

    let retained_memory = if should_measure_memory(only.as_deref()) {
        eprintln!("running realistic-fixed-cadence/memory-gateway-sequential-1000");
        measure_retained_memory(&client, &gateway_url, &tracker).await?
    } else {
        MemoryResult {
            profile: None,
            route: None,
            before_rss_bytes: None,
            after_rss_bytes: None,
            delta_bytes: None,
            after_cpu_percent: None,
            samples: 0,
            note: "Not run for a filtered benchmark; run the complete benchmark to measure exactly 1,000 gateway-only streams.",
        }
    };
    let spawn_shutdown = if only
        .as_deref()
        .is_none_or(|selected| selected == "spawn-shutdown")
    {
        eprintln!("running spawn-shutdown");
        measure_spawn_shutdown(&format!("http://{upstream_address}/v1")).await?
    } else {
        TimingSummary::default()
    };

    let mut bridge = bridge;
    bridge.shutdown();
    bridge.wait().await?;
    upstream_task.abort();
    drop(client);
    let open_fds_after = process_open_fd_count();
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
        retained_memory,
        stability: StabilityResult {
            requests_started: tracker.started.load(Ordering::Relaxed),
            requests_completed: tracker.completed.load(Ordering::Relaxed),
            active_requests_after: tracker.active(),
            max_in_flight: tracker.max_in_flight(),
            open_fds_before,
            open_fds_after,
            open_fds_delta: open_fds_before
                .zip(open_fds_after)
                .map(|(before, after)| after as i64 - before as i64),
            note: "Counts benchmark request tasks and the process file descriptors before/after shutdown; no task or connection leak is inferred when the counts are balanced.",
        },
        binary: binary_result(),
    };
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("wrote {output}");
    Ok(())
}

fn should_measure_memory(only: Option<&str>) -> bool {
    only.is_none() || selected_scenario(only, REALISTIC_PROFILE.name, "sequential-1000-streams")
}

async fn measure_retained_memory(
    client: &reqwest::Client,
    gateway_url: &str,
    tracker: &RequestTracker,
) -> Result<MemoryResult, Box<dyn std::error::Error>> {
    let body = request_body(4 * 1024, true);
    let endpoint = profile_url(gateway_url, REALISTIC_PROFILE);
    let before_rss = process_rss_bytes();
    for _ in 0..SEQUENTIAL_SAMPLES {
        benchmark::scenarios::measure_request(
            client,
            &endpoint,
            &body,
            Some(SESSION_TOKEN),
            tracker,
        )
        .await?;
    }
    let after_rss = process_rss_bytes();
    Ok(MemoryResult {
        profile: Some(REALISTIC_PROFILE.name),
        route: Some("gateway"),
        before_rss_bytes: before_rss,
        after_rss_bytes: after_rss,
        delta_bytes: before_rss
            .zip(after_rss)
            .map(|(before, after)| after as i64 - before as i64),
        after_cpu_percent: process_cpu_percent(),
        samples: SEQUENTIAL_SAMPLES,
        note: "RSS covers exactly 1,000 gateway-only realistic sequential streams; no warmup requests are included.",
    })
}

fn binary_result() -> BinaryResult {
    let current_executable_bytes = env::current_exe()
        .ok()
        .and_then(|path| fs::metadata(path).ok().map(|meta| meta.len()));
    let release_cli_bytes = fs::metadata("target/release/nan-harness")
        .ok()
        .map(|meta| meta.len());
    let baseline_cli_bytes = env::var_os("NAN_GATEWAY_BASELINE_BINARY")
        .and_then(|path| fs::metadata(path).ok().map(|meta| meta.len()));
    let (size_delta_percent, size_gate, note) = match (release_cli_bytes, baseline_cli_bytes) {
        (Some(current), Some(baseline)) if baseline > 0 => {
            let delta = (current as f64 / baseline as f64 - 1.0) * 100.0;
            let gate = if delta <= 1.0 { "pass" } else { "fail" };
            (
                Some(delta),
                gate,
                "Compared target/release/nan-harness with NAN_GATEWAY_BASELINE_BINARY; the size gate allows at most 1% growth.",
            )
        }
        (Some(_), Some(_)) => (
            None,
            "blocked-invalid-baseline",
            "NAN_GATEWAY_BASELINE_BINARY resolved to a zero-byte file; the binary-size gate is blocked.",
        ),
        (None, _) => (
            None,
            "blocked-no-current-binary",
            "No target/release/nan-harness was available; the binary-size gate is blocked.",
        ),
        (Some(_), None) => (
            None,
            "blocked-no-baseline",
            "Set NAN_GATEWAY_BASELINE_BINARY to a same-target nan-harness release binary; the binary-size gate is blocked until it is supplied.",
        ),
    };
    BinaryResult {
        current_executable_bytes,
        release_cli_bytes,
        baseline_cli_bytes,
        size_delta_percent,
        size_gate,
        note,
    }
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
                launch_id: "benchmark_spawn".to_owned(),
                provider_base_url: provider_base_url.to_owned(),
                model_id: "qwen3.6".to_owned(),
                provider_api_key: Arc::new(SecretValue::new(PROVIDER_KEY)?),
                session_token: Arc::new(SecretValue::new(SESSION_TOKEN)?),
                web_search_enabled: true,
                search_config: None,
                session_max_tokens: None,
            },
        )?;
        bridge.shutdown();
        bridge.wait().await?;
        values.push(started.elapsed());
    }
    Ok(summarize(&values))
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

#[cfg(target_os = "linux")]
fn process_open_fd_count() -> Option<u64> {
    let pid = std::process::id().to_string();
    fs::read_dir(format!("/proc/{pid}/fd"))
        .ok()
        .map(|entries| entries.filter_map(Result::ok).count() as u64)
}

#[cfg(not(target_os = "linux"))]
fn process_open_fd_count() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = Command::new("lsof")
        .args(["-a", "-p", &pid, "-Fn"])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()
        .map(|stdout| stdout.lines().filter(|line| line.starts_with('f')).count() as u64)
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
