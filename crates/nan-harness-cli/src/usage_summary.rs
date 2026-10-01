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
        nan_harness_i18n::messages::usage_partial_title(nan_harness_i18n::locale())
    } else {
        nan_harness_i18n::messages::usage_title(nan_harness_i18n::locale())
    };
    let mut output = String::new();
    if rows.len() == 1 {
        rendering::render_single(&mut output, &title, rows[0]);
    } else {
        rendering::render_multiple(&mut output, usage, &title, &rows);
    }

    let warning = partial::warning(usage, outcome);
    if !warning.is_empty() {
        let _ = write!(
            &mut output,
            "{}",
            nan_harness_i18n::messages::usage_summary_warning_usage_is_partial(
                nan_harness_i18n::locale(),
                &(warning)
            )
        );
    }
    if let Some(limit) = session_max_tokens {
        let _ = write!(
            &mut output,
            "{}", nan_harness_i18n::messages::usage_summary_budget_tokens_observed_admission_limit_in_flight_requests_may_exceed(nan_harness_i18n::locale(), &(formatting::format_number(usage.total_tokens())), &(formatting::format_number(limit))));
    }
    Some(output)
}
