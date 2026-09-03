pub(super) fn sanitize_discovery_error(body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let raw = parsed
        .pointer("/error/message")
        .or_else(|| parsed.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("NaN model discovery failed");
    raw.replace(['\r', '\n'], " ").chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize_discovery_error;

    #[test]
    fn sanitization_uses_safe_message_and_removes_line_breaks() {
        assert_eq!(
            sanitize_discovery_error(r#"{"error":{"message":"upstream\r\nfailed"}}"#),
            "upstream  failed"
        );
        assert_eq!(
            sanitize_discovery_error("not-json"),
            "NaN model discovery failed"
        );
    }

    #[test]
    fn sanitization_limits_message_length() {
        let body = format!(r#"{{"message":"{}"}}"#, "x".repeat(301));

        assert_eq!(sanitize_discovery_error(&body).len(), 300);
    }
}
