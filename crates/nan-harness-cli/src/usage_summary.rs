use nan_harness_runtime::{
    ExecutionOutcome, ExecutionReport, ModelUsageSnapshot, ProviderUsageSnapshot,
};
use std::fmt::Write as _;

mod formatting;
mod partial;
mod rendering;
mod tests;

pub(crate) fn render(report: &ExecutionReport) -> Option<String> {
    let usage = report.provider_usage.as_ref()?;
    render_snapshot(usage, report.outcome)
}

pub(crate) fn render_snapshot(
    usage: &ProviderUsageSnapshot,
    outcome: ExecutionOutcome,
) -> Option<String> {
    if usage.inference_requests() == 0 {
        return None;
    }

    let rows = rendering::sorted_rows(usage);
    let partial = partial::is_partial(usage, outcome);
    let title = if partial {
        "🔥 Tokens burned — this session (partial)"
    } else {
        "🔥 Tokens burned — this session"
    };
    let mut output = String::new();
    if rows.len() == 1 {
        rendering::render_single(&mut output, title, rows[0]);
    } else {
        rendering::render_multiple(&mut output, usage, title, &rows);
    }

    let warning = partial::warning(usage, outcome);
    if !warning.is_empty() {
        let _ = write!(&mut output, "\nwarning: Usage is partial: {}.", warning);
    }
    Some(output)
}
