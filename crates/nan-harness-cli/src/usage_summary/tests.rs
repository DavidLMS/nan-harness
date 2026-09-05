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

fn usage(input_tokens: u64, output_tokens: u64, responses_with_usage: u64) -> ModelUsageSnapshot {
    ModelUsageSnapshot {
        responses_with_usage,
        input_tokens,
        output_tokens,
        reasoning_tokens: u64::MAX,
        ..ModelUsageSnapshot::default()
    }
}

#[test]
fn stays_silent_without_inference_requests_or_a_gateway() {
    let empty = report(ExecutionOutcome::Succeeded, []);
    assert_eq!(render(&empty), None);
    let mut unsupported = empty;
    unsupported.provider_usage = None;
    assert_eq!(render(&unsupported), None);
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
fn renders_zero_tokens_without_calculating_a_percentage() {
    let report = report(
        ExecutionOutcome::Succeeded,
        [
            ("glm5.2", ModelUsageSnapshot::default()),
            (
                "qwen3.6",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    ..ModelUsageSnapshot::default()
                },
            ),
        ],
    );
    assert_eq!(
        render(&report).as_deref(),
        Some(
            "🔥 Tokens burned — this session\n\nTotal tokens: 0\nTotal requests: 1\n\nBy Model:\n🥇 qwen3.6 — 0 tokens (1 request)\n   0 input · 0 output\n   glm5.2 — token count unavailable (0 requests)"
        )
    );
}

#[test]
fn saturates_at_the_maximum_countable_token_total() {
    let report = report(
        ExecutionOutcome::Succeeded,
        [(
            "qwen3.6",
            ModelUsageSnapshot {
                responses_with_usage: 1,
                input_tokens: u64::MAX,
                output_tokens: 1,
                ..ModelUsageSnapshot::default()
            },
        )],
    );
    assert_eq!(
        render(&report).as_deref(),
        Some(
            "🔥 Tokens burned — this session\n\nqwen3.6 — 18,446,744,073,709,551,615 tokens (1 request)\n  18,446,744,073,709,551,615 input · 1 output"
        )
    );
}

#[test]
fn marks_a_failed_session_as_partial() {
    let report = report(
        ExecutionOutcome::Failed,
        [("qwen3.6", usage(1_200, 300, 1))],
    );
    assert_eq!(
        render(&report).as_deref(),
        Some(
            "🔥 Tokens burned — this session (partial)\n\nqwen3.6 — 1,500 tokens (1 request)\n  1,200 input · 300 output\nwarning: Usage is partial: session exited with a non-zero status."
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
