use super::formatting::format_number;
use super::{ExecutionOutcome, ProviderUsageSnapshot};

pub(super) fn is_partial(usage: &ProviderUsageSnapshot, outcome: ExecutionOutcome) -> bool {
    outcome != ExecutionOutcome::Succeeded
        || usage.responses_without_usage() > 0
        || usage.incomplete_responses() > 0
}

pub(super) fn warning(usage: &ProviderUsageSnapshot, outcome: ExecutionOutcome) -> String {
    let mut reasons = Vec::new();
    match outcome {
        ExecutionOutcome::Succeeded => {}
        ExecutionOutcome::Failed => {
            reasons.push("session exited with a non-zero status".to_owned());
        }
        ExecutionOutcome::Cancelled(_) => reasons.push("session was cancelled".to_owned()),
    }
    if let Some(reason) = unfinished_response_reason(
        usage.responses_without_usage(),
        "response did",
        "responses did",
        "not report token counts",
    ) {
        reasons.push(reason);
    }
    if let Some(reason) = unfinished_response_reason(
        usage.incomplete_responses(),
        "response was",
        "responses were",
        "incomplete",
    ) {
        reasons.push(reason);
    }
    reasons.join("; ")
}

fn unfinished_response_reason(
    count: u64,
    singular_verb: &str,
    plural_verb: &str,
    suffix: &str,
) -> Option<String> {
    (count > 0).then(|| {
        format!(
            "{} {} {suffix}",
            format_number(count),
            if count == 1 {
                singular_verb
            } else {
                plural_verb
            }
        )
    })
}
