use futures_util::StreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::provider::{BenchmarkProfile, profile_url, request_body};
use super::report::ScenarioResult;
use super::statistics::{Timing, route_summary, summarize_deltas};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScenarioSpec<'a> {
    pub(crate) profile: BenchmarkProfile,
    pub(crate) name: &'a str,
    pub(crate) payload_bytes: usize,
    pub(crate) stream: bool,
    pub(crate) concurrency: usize,
    pub(crate) samples: usize,
}

#[derive(Debug, Default)]
pub(crate) struct RequestTracker {
    active: AtomicUsize,
    max_in_flight: AtomicUsize,
    pub(crate) started: AtomicUsize,
    pub(crate) completed: AtomicUsize,
}

impl RequestTracker {
    fn begin(&self) -> RequestGuard<'_> {
        self.started.fetch_add(1, Ordering::Relaxed);
        let active = self.active.fetch_add(1, Ordering::Relaxed) + 1;
        self.max_in_flight.fetch_max(active, Ordering::Relaxed);
        RequestGuard { tracker: self }
    }

    pub(crate) fn active(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
    pub(crate) fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::Relaxed)
    }

    fn completed(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }
}

struct RequestGuard<'a> {
    tracker: &'a RequestTracker,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.tracker.active.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) fn selected_scenario(only: Option<&str>, profile: &str, name: &str) -> bool {
    let Some(selected) = only else {
        return true;
    };
    selected == name || selected == format!("{profile}/{name}")
}

pub(crate) async fn run(
    client: &reqwest::Client,
    baseline_url: &str,
    gateway_url: &str,
    tracker: &RequestTracker,
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
    let baseline_url = profile_url(baseline_url, profile);
    let gateway_url = profile_url(gateway_url, profile);
    let collected = Samples {
        baseline: Vec::with_capacity(samples),
        gateway: Vec::with_capacity(samples),
        baseline_wall_clock: Duration::ZERO,
        gateway_wall_clock: Duration::ZERO,
    };
    warmup(
        client,
        &baseline_url,
        &gateway_url,
        &body,
        tracker,
        profile.warmups,
    )
    .await?;
    let sample_started = Instant::now();
    let collected = if concurrency == 1 {
        collect_sequential(
            client,
            &baseline_url,
            &gateway_url,
            &body,
            tracker,
            samples,
            collected,
        )
        .await?
    } else {
        collect_concurrent(
            client,
            &baseline_url,
            &gateway_url,
            &body,
            tracker,
            spec,
            collected,
        )
        .await?
    };
    let wall_clock = sample_started.elapsed();
    Ok(build_result(
        profile,
        name,
        payload_bytes,
        concurrency,
        samples,
        wall_clock,
        &collected,
    ))
}

async fn warmup(
    client: &reqwest::Client,
    baseline_url: &str,
    gateway_url: &str,
    body: &[u8],
    tracker: &RequestTracker,
    warmups: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    for index in 0..warmups {
        if index % 2 == 0 {
            let _ = measure_request(client, baseline_url, body, None, tracker).await?;
            let _ = measure_request(
                client,
                gateway_url,
                body,
                Some(super::super::SESSION_TOKEN),
                tracker,
            )
            .await?;
        } else {
            let _ = measure_request(
                client,
                gateway_url,
                body,
                Some(super::super::SESSION_TOKEN),
                tracker,
            )
            .await?;
            let _ = measure_request(client, baseline_url, body, None, tracker).await?;
        }
    }
    Ok(())
}

struct Samples {
    baseline: Vec<Timing>,
    gateway: Vec<Timing>,
    baseline_wall_clock: Duration,
    gateway_wall_clock: Duration,
}

async fn collect_sequential(
    client: &reqwest::Client,
    baseline_url: &str,
    gateway_url: &str,
    body: &[u8],
    tracker: &RequestTracker,
    samples: usize,
    collected: Samples,
) -> Result<Samples, Box<dyn std::error::Error>> {
    let Samples {
        mut baseline,
        mut gateway,
        mut baseline_wall_clock,
        mut gateway_wall_clock,
    } = collected;
    for index in 0..samples {
        if index % 2 == 0 {
            baseline_wall_clock +=
                collect_one(client, baseline_url, body, None, tracker, &mut baseline).await?;
            gateway_wall_clock += collect_one(
                client,
                gateway_url,
                body,
                Some(super::super::SESSION_TOKEN),
                tracker,
                &mut gateway,
            )
            .await?;
        } else {
            gateway_wall_clock += collect_one(
                client,
                gateway_url,
                body,
                Some(super::super::SESSION_TOKEN),
                tracker,
                &mut gateway,
            )
            .await?;
            baseline_wall_clock +=
                collect_one(client, baseline_url, body, None, tracker, &mut baseline).await?;
        }
    }
    Ok(Samples {
        baseline,
        gateway,
        baseline_wall_clock,
        gateway_wall_clock,
    })
}

async fn collect_one(
    client: &reqwest::Client,
    endpoint: &str,
    body: &[u8],
    token: Option<&str>,
    tracker: &RequestTracker,
    output: &mut Vec<Timing>,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let started = Instant::now();
    output.push(measure_request(client, endpoint, body, token, tracker).await?);
    Ok(started.elapsed())
}

async fn collect_concurrent(
    client: &reqwest::Client,
    baseline_url: &str,
    gateway_url: &str,
    body: &[u8],
    tracker: &RequestTracker,
    spec: ScenarioSpec<'_>,
    collected: Samples,
) -> Result<Samples, Box<dyn std::error::Error>> {
    let ScenarioSpec {
        samples,
        concurrency,
        ..
    } = spec;
    let Samples {
        mut baseline,
        mut gateway,
        mut baseline_wall_clock,
        mut gateway_wall_clock,
    } = collected;
    let mut completed = 0;
    while completed < samples {
        let batch_size = concurrency.min(samples - completed);
        let gateway_first = (completed / batch_size) % 2 == 1;
        let (first, second) = if gateway_first {
            (gateway_url, baseline_url)
        } else {
            (baseline_url, gateway_url)
        };
        let first_token = gateway_first.then_some(super::super::SESSION_TOKEN);
        let second_token = (!gateway_first).then_some(super::super::SESSION_TOKEN);
        let first_tasks = (0..batch_size)
            .map(|_| measure_request(client, first, body, first_token, tracker))
            .collect::<Vec<_>>();
        let second_tasks = (0..batch_size)
            .map(|_| measure_request(client, second, body, second_token, tracker))
            .collect::<Vec<_>>();
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
    Ok(Samples {
        baseline,
        gateway,
        baseline_wall_clock,
        gateway_wall_clock,
    })
}

fn build_result(
    profile: BenchmarkProfile,
    name: &str,
    payload_bytes: usize,
    concurrency: usize,
    samples: usize,
    wall_clock: Duration,
    collected: &Samples,
) -> ScenarioResult {
    let baseline = &collected.baseline;
    let gateway = &collected.gateway;
    let baseline_duration = baseline
        .iter()
        .map(|timing| timing.completion)
        .sum::<Duration>();
    let gateway_duration = gateway
        .iter()
        .map(|timing| timing.completion)
        .sum::<Duration>();
    let baseline_bytes = baseline
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>();
    let gateway_bytes = gateway
        .iter()
        .map(|timing| timing.body_bytes as f64)
        .sum::<f64>();
    let baseline_throughput = baseline_bytes / baseline_duration.as_secs_f64().max(f64::EPSILON);
    let gateway_throughput = gateway_bytes / gateway_duration.as_secs_f64().max(f64::EPSILON);
    let baseline_wall_clock_throughput = baseline_bytes
        / collected
            .baseline_wall_clock
            .as_secs_f64()
            .max(f64::EPSILON);
    let gateway_wall_clock_throughput =
        gateway_bytes / collected.gateway_wall_clock.as_secs_f64().max(f64::EPSILON);
    ScenarioResult {
        profile: profile.name,
        name: name.to_owned(),
        payload_bytes,
        concurrency,
        warmups: profile.warmups,
        samples,
        wall_clock_ms: wall_clock.as_secs_f64() * 1_000.0,
        paired_ttft_delta_ms: summarize_deltas(baseline, gateway, |timing| timing.first_byte),
        paired_completion_delta_ms: summarize_deltas(baseline, gateway, |timing| timing.completion),
        baseline_wall_clock_throughput_bytes_per_sec: baseline_wall_clock_throughput,
        gateway_wall_clock_throughput_bytes_per_sec: gateway_wall_clock_throughput,
        wall_clock_throughput_degradation_percent: (1.0
            - gateway_wall_clock_throughput / baseline_wall_clock_throughput)
            * 100.0,
        summed_request_throughput_degradation_percent: (1.0
            - gateway_throughput / baseline_throughput)
            * 100.0,
        baseline: route_summary(baseline),
        gateway: route_summary(gateway),
    }
}

pub(crate) async fn measure_request(
    client: &reqwest::Client,
    endpoint: &str,
    body: &[u8],
    token: Option<&str>,
    tracker: &RequestTracker,
) -> Result<Timing, Box<dyn std::error::Error>> {
    let _guard = tracker.begin();
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
    let timing = Timing {
        headers,
        first_byte,
        completion: started.elapsed(),
        body_bytes,
    };
    tracker.completed();
    Ok(timing)
}

#[cfg(test)]
mod tests {
    use super::selected_scenario;

    #[test]
    fn selection_accepts_name_or_profile_name_and_rejects_other_values() {
        assert!(selected_scenario(None, "micro", "json-4k"));
        assert!(selected_scenario(Some("json-4k"), "micro", "json-4k"));
        assert!(selected_scenario(Some("micro/json-4k"), "micro", "json-4k"));
        assert!(!selected_scenario(
            Some("realistic/json-4k"),
            "micro",
            "json-4k"
        ));
    }
}
