use super::{ResponsesRequest, ToolTarget, translate};
use nan_harness_core::CodingModelProfile;
use serde_json::json;

#[test]
fn flattens_namespaces_and_preserves_reverse_routing() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model": "qwen3.6",
        "stream": true,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "search"}]
        }],
        "tools": [{
            "type": "namespace",
            "name": "web",
            "description": "Web tools",
            "tools": [{
                "type": "function",
                "name": "run",
                "description": "Search",
                "parameters": {"type": "object", "properties": {}}
            }]
        }]
    }))
    .expect("request should deserialize");

    let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
    let translated = translate(request, &model).expect("request should translate");
    assert_eq!(translated.body["tools"][0]["function"]["name"], "web__run");
    assert_eq!(
        translated.tools.target("web__run"),
        Some(&ToolTarget::Function {
            name: "run".to_owned(),
            namespace: Some("web".to_owned())
        })
    );
}

#[test]
fn converts_freeform_calls_to_chat_function_arguments() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model": "qwen3.6",
        "stream": true,
        "input": [
            {"type":"message","role":"user","content":[{"type":"input_text","text":"edit"}]},
            {"type":"custom_tool_call","name":"apply_patch","call_id":"call_1","input":"*** Begin Patch"},
            {"type":"custom_tool_call_output","call_id":"call_1","output":"Done!"}
        ],
        "tools": [{
            "type":"custom",
            "name":"apply_patch",
            "description":"Edit files",
            "format":{"type":"grammar","syntax":"lark","definition":"start: patch"}
        }]
    }))
    .expect("request should deserialize");

    let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
    let translated = translate(request, &model).expect("request should translate");
    assert_eq!(
        translated.body["messages"][1]["tool_calls"][0]["function"]["name"],
        "apply_patch"
    );
    assert!(
        translated.body["tools"][0]["function"]["description"]
            .as_str()
            .expect("custom tool description")
            .contains("split large edits")
    );
    assert_eq!(
        translated.body["tools"][0]["function"]["parameters"]["properties"]["input"]["maxLength"],
        3_000
    );
    assert_eq!(translated.body["messages"][2]["role"], "tool");
}

#[test]
fn accepts_messages_without_an_explicit_type() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model": "qwen3.6",
        "stream": true,
        "input": [{
            "role": "user",
            "content": [{"type": "input_text", "text": "inspect the workspace"}]
        }]
    }))
    .expect("request should deserialize");

    let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
    let translated = translate(request, &model).expect("request should translate");
    assert_eq!(translated.body["messages"][0]["role"], "user");
    assert_eq!(
        translated.body["messages"][0]["content"],
        "inspect the workspace"
    );
}

#[test]
fn forwards_images_for_the_new_nan_models_without_profile_gating() {
    for model_id in [
        "deepseek-v4-flash",
        "qwen3.8-flash",
        "glm5.3-flash",
        "glm5.3",
    ] {
        let request: ResponsesRequest = serde_json::from_value(json!({
            "model": model_id,
            "stream": true,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "describe this"},
                    {"type": "input_image", "image_url": "data:image/png;base64,AA=="}
                ]
            }]
        }))
        .expect("request should deserialize");
        let model = nan_harness_core::coding_model_profile(model_id)
            .expect("new NaN model should be profiled");

        let translated = translate(request, &model).expect("request should translate");
        assert_eq!(
            translated.body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AA=="
        );
    }
}

#[test]
fn translates_and_validates_native_reasoning_effort() {
    let request = |model: &str, effort: &str| {
        serde_json::from_value(json!({
            "model": model, "stream": true, "reasoning": {"effort": effort},
            "input": [{"role":"user","content":[{"type":"input_text","text":"think"}]}]
        }))
        .expect("request")
    };

    let qwen = nan_harness_core::coding_model_profile("qwen3.6").expect("model");
    let translated = translate(request("qwen3.6", "none"), &qwen).expect("toggle accepted");
    assert_eq!(
        translated.body["chat_template_kwargs"]["enable_thinking"],
        false
    );

    let deepseek = nan_harness_core::coding_model_profile("deepseek-v4-flash").expect("model");
    let translated =
        translate(request("deepseek-v4-flash", "low"), &deepseek).expect("effort accepted");
    assert_eq!(translated.body["reasoning_effort"], "low");
    let translated = translate(request("deepseek-v4-flash", "xhigh"), &deepseek)
        .expect("extended effort should use the strongest provider effort");
    assert_eq!(translated.body["reasoning_effort"], "high");
    assert!(translate(request("deepseek-v4-flash", "none"), &deepseek).is_err());

    let translated =
        translate(request("qwen3.6", "xhigh"), &qwen).expect("extended toggle accepted");
    assert_eq!(
        translated.body["chat_template_kwargs"]["enable_thinking"],
        true
    );
    let translated = translate(request("qwen3.6", "medium"), &qwen)
        .expect("positive plan-mode effort should enable toggle reasoning");
    assert_eq!(
        translated.body["chat_template_kwargs"]["enable_thinking"],
        true
    );

    let mimo = nan_harness_core::coding_model_profile("mimo-v2.5").expect("model");
    let translated =
        translate(request("mimo-v2.5", "high"), &mimo).expect("always-on state accepted");
    assert!(translated.body.get("reasoning_effort").is_none());
    assert!(translated.body.get("chat_template_kwargs").is_none());
    let translated = translate(request("mimo-v2.5", "medium"), &mimo)
        .expect("plan-mode effort should preserve always-on reasoning");
    assert!(translated.body.get("reasoning_effort").is_none());
    assert!(translated.body.get("chat_template_kwargs").is_none());

    let qwen38 = nan_harness_core::coding_model_profile("qwen3.8-flash").expect("model");
    assert!(translate(request("qwen3.8-flash", "none"), &qwen38).is_err());
    let translated = translate(request("qwen3.8-flash", "high"), &qwen38)
        .expect("always-on reasoning should be accepted");
    assert_eq!(
        translated.body["chat_template_kwargs"]["enable_thinking"],
        true
    );

    let glm53 = nan_harness_core::coding_model_profile("glm5.3-flash").expect("model");
    let translated = translate(request("glm5.3-flash", "low"), &glm53)
        .expect("effort reasoning should be accepted");
    assert_eq!(translated.body["reasoning_effort"], "low");
    assert!(translate(request("glm5.3-flash", "none"), &glm53).is_err());

    let glm53 = nan_harness_core::coding_model_profile("glm5.3").expect("model");
    let translated = translate(request("glm5.3", "medium"), &glm53).expect("effort accepted");
    assert_eq!(translated.body["reasoning_effort"], "medium");
    assert!(translate(request("glm5.3", "none"), &glm53).is_err());

    let generic = CodingModelProfile::generic("future-coding-model");
    let translated = translate(request("future-coding-model", "medium"), &generic)
        .expect("unprofiled models should use native reasoning defaults");
    assert!(translated.body.get("reasoning_effort").is_none());
    assert!(translated.body.get("chat_template_kwargs").is_none());
}

#[test]
fn replays_reasoning_content_with_a_tool_call() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model":"qwen3.6", "stream":true,
        "input":[
            {"type":"message","role":"user","content":[{"type":"input_text","text":"inspect"}]},
            {"type":"reasoning","summary":[{"type":"summary_text","text":"I should inspect first."}]},
            {"type":"function_call","name":"run","call_id":"call_1","arguments":"{}"}
        ],
        "tools":[{"type":"function","name":"run","parameters":{"type":"object"}}]
    }))
    .expect("request");
    let model = nan_harness_core::coding_model_profile("qwen3.6").expect("model");
    let translated = translate(request, &model).expect("translation");
    assert_eq!(
        translated.body["messages"][1]["reasoning_content"],
        "I should inspect first."
    );
    assert_eq!(
        translated.body["messages"][1]["tool_calls"][0]["id"],
        "call_1"
    );
}
