use nan_harness_core::DesktopTransport;
use serde_json::Value;

#[test]
fn desktop_transport_display_and_serde_names_are_stable() {
    let registry = [
        (DesktopTransport::ResponsesBridge, "responses-bridge"),
        (DesktopTransport::AnthropicBridge, "anthropic-bridge"),
        (
            DesktopTransport::ChatCompletionsGateway,
            "chat-completions-gateway",
        ),
        (
            DesktopTransport::DirectChatCompletions,
            "direct-chat-completions",
        ),
    ];

    for (transport, name) in registry {
        assert_eq!(transport.to_string(), name);
        assert_eq!(
            serde_json::to_value(transport).expect("desktop transport should serialize"),
            Value::String(name.to_owned())
        );
        assert_eq!(
            serde_json::from_value::<DesktopTransport>(Value::String(name.to_owned()))
                .expect("desktop transport name should deserialize"),
            transport
        );
    }
}
