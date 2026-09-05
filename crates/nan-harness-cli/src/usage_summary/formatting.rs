use super::ModelUsageSnapshot;
use std::fmt::Write as _;

pub(super) fn push_model_total(
    output: &mut String,
    usage: &ModelUsageSnapshot,
    percentage: Option<&str>,
) {
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

pub(super) fn request_count(count: u64) -> String {
    format!(
        "{} {}",
        format_number(count),
        if count == 1 { "request" } else { "requests" }
    )
}

pub(super) fn percentage(tokens: u64, total_tokens: u64) -> String {
    let tenths =
        (u128::from(tokens) * 1_000 + u128::from(total_tokens) / 2) / u128::from(total_tokens);
    format!("{}.{:01}%", tenths / 10, tenths % 10)
}

pub(super) fn format_number(value: u64) -> String {
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
