use nan_harness_runtime::{
    ExecutionOutcome, ExecutionReport, ModelUsageSnapshot, ProviderUsageSnapshot,
};
use std::fmt::Write as _;

mod formatting;
mod partial;
mod rendering;
#[cfg(test)]
mod tests;

pub(crate) fn render(report: &ExecutionReport) -> Option<String> {
    let usage = report.provider_usage.as_ref()?;
    render_snapshot_with_budget(usage, report.outcome, report.session_max_tokens)
}

pub(crate) fn render_snapshot_with_budget(
    usage: &ProviderUsageSnapshot,
    outcome: ExecutionOutcome,
    session_max_tokens: Option<u64>,
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
        let _ = write!(&mut output, "\nwarning: Usage is partial: {warning}.");
    }
    if let Some(limit) = session_max_tokens {
        let _ = write!(
            &mut output,
            "\nBudget: {} / {} tokens observed (admission limit; in-flight requests may exceed).",
            formatting::format_number(usage.total_tokens()),
            formatting::format_number(limit),
        );
    }
    Some(output)
}
