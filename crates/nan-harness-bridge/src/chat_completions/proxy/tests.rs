use super::{
    append_query, filter_response_headers, forward_request_headers, request_body_is_empty,
};
use axum::http::{HeaderMap, HeaderValue, header};

#[test]
fn proxy_header_boundaries_preserve_only_end_to_end_metadata() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer local"),
    );
    headers.insert(header::HOST, HeaderValue::from_static("localhost"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.append("x-client-marker", HeaderValue::from_static("one"));
    headers.append("x-client-marker", HeaderValue::from_static("two"));

    let forwarded = forward_request_headers(&headers);

    assert!(!forwarded.contains_key(header::AUTHORIZATION));
    assert!(!forwarded.contains_key(header::HOST));
    assert!(!forwarded.contains_key(header::CONTENT_LENGTH));
    assert!(!forwarded.contains_key(header::CONNECTION));
    assert_eq!(forwarded.get_all("x-client-marker").iter().count(), 2);

    let filtered = filter_response_headers(&headers);
    assert_eq!(filtered[header::AUTHORIZATION], "Bearer local");
    assert_eq!(filtered[header::HOST], "localhost");
    assert!(!filtered.contains_key(header::CONTENT_LENGTH));
    assert!(!filtered.contains_key(header::CONNECTION));
    assert_eq!(filtered.get_all("x-client-marker").iter().count(), 2);
}

#[test]
fn request_body_presence_and_query_forwarding_keep_wire_semantics() {
    assert!(request_body_is_empty(&HeaderMap::new()));

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    assert!(request_body_is_empty(&headers));
    headers.insert(
        header::TRANSFER_ENCODING,
        HeaderValue::from_static("chunked"),
    );
    assert!(!request_body_is_empty(&headers));

    assert_eq!(
        append_query(
            "https://provider.test/models".to_owned(),
            Some("owned=true")
        ),
        "https://provider.test/models?owned=true"
    );
    assert_eq!(
        append_query("https://provider.test/models".to_owned(), None),
        "https://provider.test/models"
    );
}
