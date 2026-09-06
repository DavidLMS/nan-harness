use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct Timing {
    pub(crate) headers: Duration,
    pub(crate) first_byte: Duration,
    pub(crate) completion: Duration,
    pub(crate) body_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub(crate) struct TimingSummary {
    pub(crate) samples: usize,
    pub(crate) p50_ms: f64,
    pub(crate) p95_ms: f64,
    pub(crate) p99_ms: f64,
    pub(crate) mean_ms: f64,
    pub(crate) min_ms: f64,
    pub(crate) max_ms: f64,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct RouteSummary {
    pub(crate) headers: TimingSummary,
    pub(crate) time_to_first_byte: TimingSummary,
    pub(crate) completion: TimingSummary,
    pub(crate) response_bytes: usize,
}

pub(crate) fn route_summary(values: &[Timing]) -> RouteSummary {
    RouteSummary {
        headers: summarize(
            &values
                .iter()
                .map(|timing| timing.headers)
                .collect::<Vec<_>>(),
        ),
        time_to_first_byte: summarize(
            &values
                .iter()
                .map(|timing| timing.first_byte)
                .collect::<Vec<_>>(),
        ),
        completion: summarize(
            &values
                .iter()
                .map(|timing| timing.completion)
                .collect::<Vec<_>>(),
        ),
        response_bytes: values.iter().map(|timing| timing.body_bytes).sum(),
    }
}

pub(crate) fn summarize(values: &[Duration]) -> TimingSummary {
    summarize_millis(
        values
            .iter()
            .map(Duration::as_secs_f64)
            .map(|value| value * 1_000.0)
            .collect(),
    )
}

pub(crate) fn summarize_deltas(
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

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values[index]
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use std::time::Duration;

    #[test]
    fn summary_preserves_rounding_and_empty_values() {
        let summary = summarize(&[
            Duration::from_millis(3),
            Duration::from_millis(1),
            Duration::from_millis(2),
        ]);
        assert_eq!(summary.samples, 3);
        assert_eq!(summary.p50_ms, 2.0);
        assert_eq!(summary.p95_ms, 3.0);
        assert_eq!(summary.p99_ms, 3.0);
        assert_eq!(summary.mean_ms, 2.0);
        assert_eq!(summary.min_ms, 1.0);
        assert_eq!(summary.max_ms, 3.0);
        assert_eq!(summarize(&[]).samples, 0);
        assert_eq!(summarize(&[]).mean_ms, 0.0);
    }
}
