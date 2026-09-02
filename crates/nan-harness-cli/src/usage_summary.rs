use nan_harness_runtime::{
    ExecutionOutcome, ExecutionReport, ModelUsageSnapshot, ProviderUsageSnapshot,
};
use std::fmt::Write as _;

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

    let mut rows = usage.models.iter().collect::<Vec<_>>();
    rows.sort_by(|(left_model, left), (right_model, right)| {
        right
            .total_tokens()
            .cmp(&left.total_tokens())
            .then_with(|| (right.responses_with_usage > 0).cmp(&(left.responses_with_usage > 0)))
            .then_with(|| left_model.cmp(right_model))
    });

    let partial = outcome != ExecutionOutcome::Succeeded
        || usage.responses_without_usage() > 0
        || usage.incomplete_responses() > 0;
    let title = if partial {
        "🔥 Tokens burned — this session (partial)"
    } else {
        "🔥 Tokens burned — this session"
    };
    let mut output = String::new();
    if rows.len() == 1 {
        let (model, model_usage) = rows[0];
        let _ = writeln!(&mut output, "{title}\n");
        let _ = write!(&mut output, "{model} — ");
        push_model_total(&mut output, model_usage, None);
        if model_usage.responses_with_usage > 0 {
            let _ = write!(
                &mut output,
                "\n  {} input · {} output",
                format_number(model_usage.input_tokens),
                format_number(model_usage.output_tokens)
            );
        }
    } else {
        let _ = writeln!(&mut output, "{title}\n");
        let total_tokens = usage.total_tokens();
        if usage.responses_with_usage() > 0 {
            let _ = writeln!(&mut output, "Total tokens: {}", format_number(total_tokens));
        } else {
            output.push_str("Total tokens: token count unavailable\n");
        }
        let _ = writeln!(
            &mut output,
            "Total requests: {}\n",
            format_number(usage.inference_requests())
        );
        output.push_str("By Model:\n");
        let total_is_observed = usage.responses_with_usage() > 0 && total_tokens > 0;
        for (index, (model, model_usage)) in rows.iter().enumerate() {
            let medal = match (model_usage.responses_with_usage > 0, index) {
                (true, 0) => "🥇",
                (true, 1) => "🥈",
                (true, 2) => "🥉",
                _ => "  ",
            };
            let _ = write!(&mut output, "{medal} {model} — ");
            let percentage = (total_is_observed && model_usage.responses_with_usage > 0)
                .then(|| percentage(model_usage.total_tokens(), total_tokens));
            push_model_total(&mut output, model_usage, percentage.as_deref());
            if model_usage.responses_with_usage > 0 {
                let _ = write!(
                    &mut output,
                    "\n   {} input · {} output",
                    format_number(model_usage.input_tokens),
                    format_number(model_usage.output_tokens)
                );
            }
            if index + 1 < rows.len() {
                output.push('\n');
            }
        }
    }

    let warning = partial_warning(usage, outcome);
    if !warning.is_empty() {
        let _ = write!(&mut output, "\nwarning: Usage is partial: {warning}.");
    }
    Some(output)
}

fn push_model_total(output: &mut String, usage: &ModelUsageSnapshot, percentage: Option<&str>) {
    if usage.responses_with_usage == 0 {
        let _ = write!(
            output,
            "token count unavailable ({})",
            request_count(usage.inference_requests())
        );
        return;
    }
    let _ = write!(
        output,
        "{} tokens ({}",
        format_number(usage.total_tokens()),
        request_count(usage.inference_requests())
    );
    if let Some(percentage) = percentage {
        let _ = write!(output, ", {percentage}");
    }
    output.push(')');
}

fn request_count(count: u64) -> String {
    format!(
        "{} {}",
        format_number(count),
        if count == 1 { "request" } else { "requests" }
    )
}

fn percentage(tokens: u64, total_tokens: u64) -> String {
    let tenths =
        (u128::from(tokens) * 1_000 + u128::from(total_tokens) / 2) / u128::from(total_tokens);
    format!("{}.{:01}%", tenths / 10, tenths % 10)
}

fn partial_warning(usage: &ProviderUsageSnapshot, outcome: ExecutionOutcome) -> String {
    let mut reasons = Vec::new();
    match outcome {
        ExecutionOutcome::Succeeded => {}
        ExecutionOutcome::Failed => {
            reasons.push("session exited with a non-zero status".to_owned());
        }
        ExecutionOutcome::Cancelled(_) => reasons.push("session was cancelled".to_owned()),
    }
    let without_usage = usage.responses_without_usage();
    if without_usage > 0 {
        reasons.push(format!(
            "{} {} not report token counts",
            format_number(without_usage),
            if without_usage == 1 {
                "response did"
            } else {
                "responses did"
            }
        ));
    }
    let incomplete = usage.incomplete_responses();
    if incomplete > 0 {
        reasons.push(format!(
            "{} {} incomplete",
            format_number(incomplete),
            if incomplete == 1 {
                "response was"
            } else {
                "responses were"
            }
        ));
    }
    reasons.join("; ")
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::render;
    use nan_harness_runtime::{
        ExecutionOutcome, ExecutionReport, ModelUsageSnapshot, ProviderUsageSnapshot, SignalKind,
    };
    use std::collections::BTreeMap;

    fn report(
        outcome: ExecutionOutcome,
        models: impl IntoIterator<Item = (&'static str, ModelUsageSnapshot)>,
    ) -> ExecutionReport {
        ExecutionReport {
            outcome,
            exit_code: 0,
            temporary_root: None,
            selected_model: None,
            selected_reasoning: None,
            bridge_diagnostics: Vec::new(),
            provider_usage: Some(ProviderUsageSnapshot {
                models: models
                    .into_iter()
                    .map(|(model, usage)| (model.to_owned(), usage))
                    .collect::<BTreeMap<_, _>>(),
            }),
        }
    }

    #[test]
    fn renders_one_model_as_a_compact_summary_without_duplicate_totals() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [("qwen3.6", usage(44_944, 53, 1))],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session\n\nqwen3.6 — 44,997 tokens (1 request)\n  44,944 input · 53 output"
            )
        );
        let rendered = render(&report).expect("usage should be rendered");
        assert!(!rendered.contains("Total tokens"));
        assert!(!rendered.contains("Total requests"));
        assert!(!rendered.contains("By Model"));
        assert!(!rendered.contains('%'));
    }

    #[test]
    fn renders_multiple_models_with_totals_requests_medals_and_percentages() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [
                ("zeta", usage(10_000, 987, 1)),
                ("alpha", usage(10_000, 500, 1)),
                ("qwen3.6", usage(22_458, 54, 1)),
            ],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session\n\nTotal tokens: 43,999\nTotal requests: 3\n\nBy Model:\n🥇 qwen3.6 — 22,512 tokens (1 request, 51.2%)\n   22,458 input · 54 output\n🥈 zeta — 10,987 tokens (1 request, 25.0%)\n   10,000 input · 987 output\n🥉 alpha — 10,500 tokens (1 request, 23.9%)\n   10,000 input · 500 output"
            )
        );
    }

    #[test]
    fn sorts_equal_totals_by_model_identifier_and_pluralizes_requests() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [
                ("zeta", usage(10_000, 500, 2)),
                ("alpha", usage(10_000, 500, 2)),
            ],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session\n\nTotal tokens: 21,000\nTotal requests: 4\n\nBy Model:\n🥇 alpha — 10,500 tokens (2 requests, 50.0%)\n   10,000 input · 500 output\n🥈 zeta — 10,500 tokens (2 requests, 50.0%)\n   10,000 input · 500 output"
            )
        );
    }

    #[test]
    fn renders_partial_reasons_and_unavailable_rows_with_pluralization() {
        let report = report(
            ExecutionOutcome::Cancelled(SignalKind::Interrupt),
            [
                (
                    "qwen3.6",
                    ModelUsageSnapshot {
                        responses_with_usage: 1,
                        responses_without_usage: 1,
                        incomplete_responses: 1,
                        input_tokens: 12_400,
                        output_tokens: 830,
                        reasoning_tokens: 800,
                    },
                ),
                (
                    "glm5.2",
                    ModelUsageSnapshot {
                        responses_without_usage: 1,
                        incomplete_responses: 1,
                        ..ModelUsageSnapshot::default()
                    },
                ),
            ],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session (partial)\n\nTotal tokens: 13,230\nTotal requests: 5\n\nBy Model:\n🥇 qwen3.6 — 13,230 tokens (3 requests, 100.0%)\n   12,400 input · 830 output\n   glm5.2 — token count unavailable (2 requests)\nwarning: Usage is partial: session was cancelled; 2 responses did not report token counts; 2 responses were incomplete."
            )
        );
    }

    #[test]
    fn renders_singular_partial_warning_and_request_label() {
        let report = report(
            ExecutionOutcome::Cancelled(SignalKind::Interrupt),
            [(
                "qwen3.6",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    responses_without_usage: 1,
                    incomplete_responses: 1,
                    input_tokens: 12_400,
                    output_tokens: 830,
                    reasoning_tokens: 500,
                },
            )],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session (partial)\n\nqwen3.6 — 13,230 tokens (3 requests)\n  12,400 input · 830 output\nwarning: Usage is partial: session was cancelled; 1 response did not report token counts; 1 response was incomplete."
            )
        );
    }

    #[test]
    fn reports_unavailable_tokens_when_no_response_has_usage() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [
                (
                    "qwen3.6",
                    ModelUsageSnapshot {
                        responses_without_usage: 1,
                        ..ModelUsageSnapshot::default()
                    },
                ),
                (
                    "glm5.2",
                    ModelUsageSnapshot {
                        incomplete_responses: 1,
                        ..ModelUsageSnapshot::default()
                    },
                ),
            ],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "🔥 Tokens burned — this session (partial)\n\nTotal tokens: token count unavailable\nTotal requests: 2\n\nBy Model:\n   glm5.2 — token count unavailable (1 request)\n   qwen3.6 — token count unavailable (1 request)\nwarning: Usage is partial: 1 response did not report token counts; 1 response was incomplete."
            )
        );
    }

    #[test]
    fn stays_silent_without_inference_requests_or_a_gateway() {
        let empty = report(ExecutionOutcome::Succeeded, []);
        assert_eq!(render(&empty), None);
        let mut unsupported = empty;
        unsupported.provider_usage = None;
        assert_eq!(render(&unsupported), None);
    }

    fn usage(
        input_tokens: u64,
        output_tokens: u64,
        responses_with_usage: u64,
    ) -> ModelUsageSnapshot {
        ModelUsageSnapshot {
            responses_with_usage,
            input_tokens,
            output_tokens,
            reasoning_tokens: u64::MAX,
            ..ModelUsageSnapshot::default()
        }
    }
}
