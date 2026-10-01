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
            "{}",
            nan_harness_i18n::messages::formatting_token_count_unavailable(
                nan_harness_i18n::locale(),
                &(request_count(usage.inference_requests()))
            )
        );
        return;
    }
    let tokens = format_number(usage.total_tokens());
    let requests = request_count(usage.inference_requests());
    let message = if let Some(percentage) = percentage {
        nan_harness_i18n::messages::usage_model_share(
            nan_harness_i18n::locale(),
            percentage,
            &requests,
            &tokens,
        )
    } else {
        nan_harness_i18n::messages::usage_model_total(
            nan_harness_i18n::locale(),
            &requests,
            &tokens,
        )
    };
    output.push_str(&message);
}

pub(super) fn request_count(count: u64) -> String {
    nan_harness_i18n::messages::usage_requests(
        nan_harness_i18n::locale(),
        count,
        &format_number(count),
    )
}

pub(super) fn percentage(tokens: u64, total_tokens: u64) -> String {
    percentage_for(nan_harness_i18n::locale(), tokens, total_tokens)
}

fn percentage_for(locale: nan_harness_i18n::Locale, tokens: u64, total_tokens: u64) -> String {
    let tenths =
        (u128::from(tokens) * 1_000 + u128::from(total_tokens) / 2) / u128::from(total_tokens);
    let separator = if locale == nan_harness_i18n::Locale::Es {
        ','
    } else {
        '.'
    };
    format!("{}{separator}{:01}%", tenths / 10, tenths % 10)
}

pub(super) fn format_number(value: u64) -> String {
    format_number_for(nan_harness_i18n::locale(), value)
}

fn format_number_for(locale: nan_harness_i18n::Locale, value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(if locale == nan_harness_i18n::Locale::Es {
                '.'
            } else {
                ','
            });
        }
        formatted.push(character);
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::{format_number_for, percentage_for};
    use nan_harness_i18n::Locale;

    #[test]
    fn spanish_numeric_presentation_preserves_integer_precision() {
        assert_eq!(format_number_for(Locale::Es, 0), "0");
        assert_eq!(format_number_for(Locale::Es, 1_234_567), "1.234.567");
        assert_eq!(
            format_number_for(Locale::Es, u64::MAX),
            "18.446.744.073.709.551.615"
        );
        assert_eq!(
            format_number_for(Locale::En, u64::MAX),
            "18,446,744,073,709,551,615"
        );
        assert_eq!(percentage_for(Locale::Es, u64::MAX / 2, u64::MAX), "50,0%");
        assert_eq!(percentage_for(Locale::En, u64::MAX, u64::MAX), "100.0%");
    }
}
