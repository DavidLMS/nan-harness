use serde::Serialize;

use super::provider::BenchmarkProfile;
use super::statistics::{RouteSummary, TimingSummary};

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) metadata: Metadata,
    pub(crate) profiles: Vec<ProfileMetadata>,
    pub(crate) scenarios: Vec<ScenarioResult>,
    pub(crate) spawn_shutdown: TimingSummary,
    pub(crate) retained_memory: MemoryResult,
    pub(crate) stability: StabilityResult,
    pub(crate) binary: BinaryResult,
}

#[derive(Debug, Serialize)]
pub(crate) struct Metadata {
    pub(crate) mode: &'static str,
    pub(crate) host: String,
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) rustc: String,
    pub(crate) command: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileMetadata {
    pub(crate) name: &'static str,
    pub(crate) initial_delay_ms: u64,
    pub(crate) event_delay_ms: u64,
    pub(crate) warmups: usize,
    pub(crate) samples: usize,
    pub(crate) sequential_samples: usize,
    pub(crate) note: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScenarioResult {
    pub(crate) profile: &'static str,
    pub(crate) name: String,
    pub(crate) payload_bytes: usize,
    pub(crate) concurrency: usize,
    pub(crate) warmups: usize,
    pub(crate) samples: usize,
    pub(crate) wall_clock_ms: f64,
    pub(crate) baseline: RouteSummary,
    pub(crate) gateway: RouteSummary,
    pub(crate) paired_ttft_delta_ms: TimingSummary,
    pub(crate) paired_completion_delta_ms: TimingSummary,
    pub(crate) baseline_wall_clock_throughput_bytes_per_sec: f64,
    pub(crate) gateway_wall_clock_throughput_bytes_per_sec: f64,
    pub(crate) wall_clock_throughput_degradation_percent: f64,
    pub(crate) summed_request_throughput_degradation_percent: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct MemoryResult {
    pub(crate) profile: Option<&'static str>,
    pub(crate) route: Option<&'static str>,
    pub(crate) before_rss_bytes: Option<u64>,
    pub(crate) after_rss_bytes: Option<u64>,
    pub(crate) delta_bytes: Option<i64>,
    pub(crate) after_cpu_percent: Option<f64>,
    pub(crate) samples: usize,
    pub(crate) note: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct BinaryResult {
    pub(crate) current_executable_bytes: Option<u64>,
    pub(crate) release_cli_bytes: Option<u64>,
    pub(crate) baseline_cli_bytes: Option<u64>,
    pub(crate) size_delta_percent: Option<f64>,
    pub(crate) size_gate: &'static str,
    pub(crate) note: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct StabilityResult {
    pub(crate) requests_started: usize,
    pub(crate) requests_completed: usize,
    pub(crate) active_requests_after: usize,
    pub(crate) max_in_flight: usize,
    pub(crate) open_fds_before: Option<u64>,
    pub(crate) open_fds_after: Option<u64>,
    pub(crate) open_fds_delta: Option<i64>,
    pub(crate) note: &'static str,
}

pub(crate) fn profile_metadata(profile: BenchmarkProfile) -> ProfileMetadata {
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
