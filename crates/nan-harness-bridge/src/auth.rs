use axum::http::HeaderMap;
use nan_harness_core::SecretValue;
use subtle::ConstantTimeEq;

pub(crate) fn is_authorized(headers: &HeaderMap, expected: &SecretValue) -> bool {
    let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };

    expected.with_secret(|expected| {
        token.len() == expected.len() && bool::from(token.as_bytes().ct_eq(expected.as_bytes()))
    })
}
