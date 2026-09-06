use super::translate;
use nan_harness_core::CodingModelProfile;
use serde_json::{Value, json};

fn translated_tool_messages(content: &Value) -> Value {
    let request = json!({
        "prompt": [
            {"role": "user", "content": "synthetic fx request"},
            {"role": "tool", "content": content},
        ]
    });
    let model = CodingModelProfile::generic("synthetic-fx-model");

    translate(&request, &model).expect("fx request should translate")
}

fn translated_tool_result(output: &Value) -> Value {
    translated_tool_messages(&json!([{
        "type": "tool-result",
        "toolCallId": "call-under-test",
        "output": output,
    }]))
}

fn tool_content(translated: &Value) -> &str {
    translated["messages"][1]["content"]
        .as_str()
        .expect("tool message should have string content")
}

#[test]
fn text_tool_output_unwraps_exact_string() {
    let translated = translated_tool_result(&json!({
        "type": "text",
        "value": "Preserved \"quotes\"\nnewline"
    }));

    assert_eq!(
        translated["messages"],
        json!([
            {"role": "user", "content": "synthetic fx request"},
            {
                "role": "tool",
                "tool_call_id": "call-under-test",
                "content": "Preserved \"quotes\"\nnewline",
            },
        ])
    );
}

#[test]
fn malformed_text_tool_output_falls_back_to_empty_string() {
    let cases = [
        ("missing value", json!({"type": "text"})),
        ("non-string value", json!({"type": "text", "value": 42})),
    ];

    for (name, output) in cases {
        let translated = translated_tool_result(&output);
        let tool_message = &translated["messages"][1];

        assert_eq!(tool_message["tool_call_id"], "call-under-test");
        assert_eq!(
            tool_message["content"], "",
            "{name} should use the empty-string fallback"
        );
    }
}

#[test]
fn structured_tool_output_serializes_as_json() {
    let non_text_object = translated_tool_result(&json!({
        "type": "json",
        "value": {"answer": true},
    }));
    let non_text_content = tool_content(&non_text_object);
    let non_text_serialized: Value =
        serde_json::from_str(non_text_content).expect("object output should serialize as JSON");

    assert_eq!(
        non_text_serialized,
        json!({"type": "json", "value": {"answer": true}})
    );

    let raw_string = translated_tool_result(&json!("synthetic raw output"));
    assert_eq!(tool_content(&raw_string), r#""synthetic raw output""#);

    let null_output = translated_tool_result(&Value::Null);
    assert_eq!(tool_content(&null_output), "null");

    let array = translated_tool_result(&json!(["alpha", {"answer": true}]));
    let array_content = tool_content(&array);
    let array_serialized: Value =
        serde_json::from_str(array_content).expect("array output should serialize as JSON");
    assert_eq!(array_serialized, json!(["alpha", {"answer": true}]));
}

#[test]
fn absent_tool_output_translates_to_empty_string() {
    let translated = translated_tool_messages(&json!([{
        "type": "tool-result",
        "toolCallId": "call-under-test",
    }]));

    assert_eq!(
        translated["messages"],
        json!([
            {"role": "user", "content": "synthetic fx request"},
            {
                "role": "tool",
                "tool_call_id": "call-under-test",
                "content": "",
            },
        ])
    );
}

#[test]
fn tool_results_keep_call_order_and_skip_unrelated_parts() {
    let translated = translated_tool_messages(&json!([
        {"type": "text", "text": "ignored before results"},
        {
            "type": "tool-result",
            "toolCallId": "call-first",
            "output": {"type": "text", "value": "first\nresult"},
        },
        {"type": "text", "text": "ignored between results"},
        {"type": "tool-result", "output": "synthetic raw output"},
        {"type": "text", "text": "ignored after results"},
    ]));

    assert_eq!(
        translated["messages"],
        json!([
            {"role": "user", "content": "synthetic fx request"},
            {
                "role": "tool",
                "tool_call_id": "call-first",
                "content": "first\nresult",
            },
            {
                "role": "tool",
                "tool_call_id": "fx_tool_call",
                "content": r#""synthetic raw output""#,
            },
        ])
    );
}
