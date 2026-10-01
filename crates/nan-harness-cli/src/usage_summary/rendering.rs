use super::formatting::{format_number, percentage, push_model_total};
use super::{ModelUsageSnapshot, ProviderUsageSnapshot};
use std::fmt::Write as _;

type UsageRow<'a> = (&'a String, &'a ModelUsageSnapshot);

pub(super) fn sorted_rows(usage: &ProviderUsageSnapshot) -> Vec<UsageRow<'_>> {
    let mut rows = usage.models.iter().collect::<Vec<_>>();
    rows.sort_by(|(left_model, left), (right_model, right)| {
        right
            .total_tokens()
            .cmp(&left.total_tokens())
            .then_with(|| (right.responses_with_usage > 0).cmp(&(left.responses_with_usage > 0)))
            .then_with(|| left_model.cmp(right_model))
    });
    rows
}

pub(super) fn render_single(output: &mut String, title: &str, (model, model_usage): UsageRow<'_>) {
    let _ = writeln!(output, "{title}\n");
    let _ = write!(output, "{model} — ");
    push_model_total(output, model_usage, None);
    if model_usage.responses_with_usage > 0 {
        let _ = write!(
            output,
            "{}",
            nan_harness_i18n::messages::rendering_input_output(
                nan_harness_i18n::locale(),
                &(format_number(model_usage.input_tokens)),
                &(format_number(model_usage.output_tokens))
            )
        );
    }
}

pub(super) fn render_multiple(
    output: &mut String,
    usage: &ProviderUsageSnapshot,
    title: &str,
    rows: &[UsageRow<'_>],
) {
    let _ = writeln!(output, "{title}\n");
    let total_tokens = usage.total_tokens();
    if usage.responses_with_usage() > 0 {
        let _ = writeln!(
            output,
            "{}",
            nan_harness_i18n::messages::rendering_total_tokens(
                nan_harness_i18n::locale(),
                &(format_number(total_tokens))
            )
        );
    } else {
        output.push_str(&nan_harness_i18n::messages::usage_no_total(
            nan_harness_i18n::locale(),
        ));
    }
    let _ = writeln!(
        output,
        "{}",
        nan_harness_i18n::messages::rendering_total_requests(
            nan_harness_i18n::locale(),
            &(format_number(usage.inference_requests()))
        )
    );
    output.push_str(&nan_harness_i18n::messages::usage_by_model(
        nan_harness_i18n::locale(),
    ));
    let total_is_observed = usage.responses_with_usage() > 0 && total_tokens > 0;
    for (index, &(model, model_usage)) in rows.iter().enumerate() {
        let medal = match (model_usage.responses_with_usage > 0, index) {
            (true, 0) => "🥇",
            (true, 1) => "🥈",
            (true, 2) => "🥉",
            _ => "  ",
        };
        let _ = write!(output, "{medal} {model} — ");
        let percentage = (total_is_observed && model_usage.responses_with_usage > 0)
            .then(|| percentage(model_usage.total_tokens(), total_tokens));
        push_model_total(output, model_usage, percentage.as_deref());
        if model_usage.responses_with_usage > 0 {
            let _ = write!(
                output,
                "{}",
                nan_harness_i18n::messages::rendering_input_output_details(
                    nan_harness_i18n::locale(),
                    &(format_number(model_usage.input_tokens)),
                    &(format_number(model_usage.output_tokens))
                )
            );
        }
        if index + 1 < rows.len() {
            output.push('\n');
        }
    }
}
