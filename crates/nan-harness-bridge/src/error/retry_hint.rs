//! A provider retry hint, never an arbitrary response-header passthrough.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RetryHint {
    #[default]
    Default,
    NoRetry,
}
impl RetryHint {
    pub(crate) fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        let mut values = headers.get_all("x-should-retry").iter();
        if values
            .next()
            .is_some_and(|value| value.as_bytes() == b"false")
            && values.next().is_none()
        {
            Self::NoRetry
        } else {
            Self::Default
        }
    }
}
#[cfg(test)]
mod tests {
    use super::RetryHint;
    #[test]
    fn only_one_exact_false_is_authoritative() {
        for values in [
            vec![],
            vec!["true"],
            vec!["False"],
            vec!["false,true"],
            vec!["false "],
            vec!["false", "false"],
            vec!["false", "true"],
        ] {
            let mut headers = reqwest::header::HeaderMap::new();
            for value in values {
                headers.append("x-should-retry", value.parse().unwrap());
            }
            assert_eq!(RetryHint::from_headers(&headers), RetryHint::Default);
        }
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-should-retry", "false".parse().unwrap());
        assert_eq!(RetryHint::from_headers(&headers), RetryHint::NoRetry);
    }
}
