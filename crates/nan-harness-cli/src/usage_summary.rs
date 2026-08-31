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
            .then_with(|| left_model.cmp(right_model))
    });

    let partial = outcome != ExecutionOutcome::Succeeded
        || usage.responses_without_usage() > 0
        || usage.incomplete_responses() > 0;
    let label = if partial {
        "NaN usage (provider-reported, partial)"
    } else {
        "NaN usage (provider-reported)"
    };
    let mut output = String::new();
    if rows.len() == 1 {
        let (model, model_usage) = rows[0];
        let _ = write!(&mut output, "{label} · {model} · ");
        push_counts(&mut output, model_usage);
    } else {
        let _ = writeln!(&mut output, "{label}:");
        for (index, (model, model_usage)) in rows.iter().enumerate() {
            let _ = write!(&mut output, "  {model} · ");
            push_counts(&mut output, model_usage);
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

fn push_counts(output: &mut String, usage: &ModelUsageSnapshot) {
    if usage.responses_with_usage == 0 {
        output.push_str("unavailable");
        return;
    }
    let _ = write!(
        output,
        "{} input · {} output",
        format_number(usage.input_tokens),
        format_number(usage.output_tokens)
    );
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
    fn renders_one_model_with_exact_ascii_separators() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [("qwen3.6", usage(184_231, 9_104))],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some("NaN usage (provider-reported) · qwen3.6 · 184,231 input · 9,104 output")
        );
    }

    #[test]
    fn renders_multiple_models_by_total_then_identifier() {
        let report = report(
            ExecutionOutcome::Succeeded,
            [
                ("zeta", usage(10_000, 500)),
                ("alpha", usage(10_000, 500)),
                ("qwen3.6", usage(184_231, 9_104)),
            ],
        );
        assert_eq!(
            render(&report).as_deref(),
            Some(
                "NaN usage (provider-reported):\n  qwen3.6 · 184,231 input · 9,104 output\n  alpha · 10,000 input · 500 output\n  zeta · 10,000 input · 500 output"
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
                "NaN usage (provider-reported, partial):\n  qwen3.6 · 12,400 input · 830 output\n  glm5.2 · unavailable\nwarning: Usage is partial: session was cancelled; 2 responses did not report token counts; 2 responses were incomplete."
            )
        );
    }

    #[test]
    fn renders_the_exact_singular_partial_warning() {
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
                "NaN usage (provider-reported, partial) · qwen3.6 · 12,400 input · 830 output\nwarning: Usage is partial: session was cancelled; 1 response did not report token counts; 1 response was incomplete."
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

    fn usage(input_tokens: u64, output_tokens: u64) -> ModelUsageSnapshot {
        ModelUsageSnapshot {
            responses_with_usage: 1,
            input_tokens,
            output_tokens,
            reasoning_tokens: output_tokens,
            ..ModelUsageSnapshot::default()
        }
    }
}
