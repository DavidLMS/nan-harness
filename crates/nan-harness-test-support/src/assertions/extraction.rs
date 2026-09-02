use serde_json::Value;

pub(super) fn normalized_tool_result_id(identifier: &str) -> String {
    identifier
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

pub(super) fn extract_tool_calls(request: &Value) -> Vec<(String, String, Value)> {
    request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|call| {
                    let id = call.get("id")?.as_str()?.to_owned();
                    let function = call.get("function")?;
                    let name = function.get("name")?.as_str()?.to_owned();
                    let arguments = function.get("arguments")?;
                    let input = arguments.as_str().map_or_else(
                        || Some(arguments.clone()),
                        |arguments| serde_json::from_str(arguments).ok(),
                    )?;
                    Some((id, name, input))
                })
        })
        .collect()
}

pub(super) fn unique_tool_calls(requests: &[Value]) -> Vec<(String, String, Value)> {
    let mut calls = Vec::new();
    for call in requests.iter().flat_map(extract_tool_calls) {
        if calls.iter().any(|existing| existing == &call) {
            continue;
        }
        calls.push(call);
    }
    calls
}

pub(super) fn extract_tool_results(request: &Value) -> Vec<(String, Value)> {
    request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .filter_map(|message| {
            Some((
                message.get("tool_call_id")?.as_str()?.to_owned(),
                message.get("content")?.clone(),
            ))
        })
        .collect()
}

pub(super) fn unique_tool_results(requests: &[Value]) -> Vec<(String, Value)> {
    let mut results = Vec::new();
    for result in requests.iter().flat_map(extract_tool_results) {
        if results.iter().any(|existing| existing == &result) {
            continue;
        }
        results.push(result);
    }
    results
}

pub(super) fn request_has_tool_traffic(request: &Value) -> bool {
    request_has_tool_calls(request)
        || request
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
            })
}

pub(super) fn request_has_tool_calls(request: &Value) -> bool {
    request
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty())
            })
        })
}

pub(super) fn value_is_error(value: &Value) -> bool {
    let text = value
        .as_str()
        .map(str::trim_start)
        .unwrap_or_default()
        .to_ascii_lowercase();
    text.starts_with("error")
        || text.starts_with("<system>error:")
        || value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

pub(super) fn value_contains_pair(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get(key).and_then(Value::as_str) == Some(expected)
                || object
                    .values()
                    .any(|value| value_contains_pair(value, key, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_pair(value, key, expected)),
        _ => false,
    }
}

pub(super) fn find_all_tool_uses(value: &Value) -> Vec<(String, String)> {
    let mut matches = Vec::new();
    collect_all_tool_uses(value, &mut matches);
    matches
}

fn collect_all_tool_uses(value: &Value, matches: &mut Vec<(String, String)>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_use")
                && let (Some(id), Some(name)) = (
                    object.get("id").and_then(Value::as_str),
                    object.get("name").and_then(Value::as_str),
                )
            {
                matches.push((id.to_owned(), name.to_owned()));
            }
            for value in object.values() {
                collect_all_tool_uses(value, matches);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_all_tool_uses(value, matches);
            }
        }
        _ => {}
    }
}

pub(super) fn find_all_tool_results(value: &Value) -> Vec<(String, bool)> {
    let mut matches = Vec::new();
    collect_all_tool_results(value, &mut matches);
    matches
}

fn collect_all_tool_results(value: &Value, matches: &mut Vec<(String, bool)>) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("tool_result")
                && let Some(id) = ["tool_use_id", "tool_call_id"]
                    .into_iter()
                    .find_map(|key| object.get(key).and_then(Value::as_str))
            {
                matches.push((
                    id.to_owned(),
                    object
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                ));
            }
            for value in object.values() {
                collect_all_tool_results(value, matches);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_all_tool_results(value, matches);
            }
        }
        _ => {}
    }
}

pub(super) fn value_contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text.contains(expected),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_string(value, expected)),
        Value::Object(object) => object
            .values()
            .any(|value| value_contains_string(value, expected)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(super) fn value_contains_bool(value: &Value, key: &str, expected: bool) -> bool {
    match value {
        Value::Object(object) => {
            object.get(key).and_then(Value::as_bool) == Some(expected)
                || object
                    .values()
                    .any(|value| value_contains_bool(value, key, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_bool(value, key, expected)),
        _ => false,
    }
}
