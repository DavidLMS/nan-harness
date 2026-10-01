use super::formatting::format_number;
use super::{ExecutionOutcome, ProviderUsageSnapshot};

pub(super) fn is_partial(usage: &ProviderUsageSnapshot, outcome: ExecutionOutcome) -> bool {
    outcome != ExecutionOutcome::Succeeded
        || usage.responses_without_usage() > 0
        || usage.incomplete_responses() > 0
}

pub(super) fn warning(usage: &ProviderUsageSnapshot, outcome: ExecutionOutcome) -> String {
    use nan_harness_i18n::{locale, messages};
    let mut reasons = Vec::new();
    match outcome {
        ExecutionOutcome::Succeeded => {}
        ExecutionOutcome::Failed => reasons.push(messages::usage_failed(locale())),
        ExecutionOutcome::Cancelled(_) => reasons.push(messages::usage_cancelled(locale())),
    }
    let missing = usage.responses_without_usage();
    if missing > 0 {
        reasons.push(messages::usage_missing_counts(
            locale(),
            missing,
            &format_number(missing),
        ));
    }
    let incomplete = usage.incomplete_responses();
    if incomplete > 0 {
        reasons.push(messages::usage_incomplete(
            locale(),
            incomplete,
            &format_number(incomplete),
        ));
    }
    reasons.join("; ")
}
