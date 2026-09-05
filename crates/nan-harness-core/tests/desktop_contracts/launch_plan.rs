use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport};
use serde_json::Value;
use std::path::PathBuf;

#[test]
fn desktop_launch_plan_defaults_are_safe_for_every_harness_transport_pair() {
    for harness in DesktopHarnessKind::ALL {
        for transport in [
            DesktopTransport::ResponsesBridge,
            DesktopTransport::AnthropicBridge,
            DesktopTransport::ChatCompletionsGateway,
            DesktopTransport::DirectChatCompletions,
        ] {
            let plan = DesktopLaunchPlan::new(harness, transport);

            assert_eq!(plan.schema_version, 1);
            assert_eq!(plan.harness, harness);
            assert_eq!(plan.transport, transport);
            assert!(plan.experimental);
            assert_eq!(plan.platform, std::env::consts::OS);
            assert_eq!(plan.executable, None);
            assert_eq!(plan.selected_model, None);
            assert_eq!(plan.auxiliary_model, None);
            assert!(matches!(
                plan.web_search_policy,
                nan_harness_core::WebSearchPolicy::Auto
            ));
            assert!(!plan.persistent_profile);
            assert!(!plan.private_diagnostics);
            assert!(!plan.restore_only);
            assert_eq!(plan.native_arguments, Vec::<String>::new());
        }
    }
}

#[test]
fn default_desktop_launch_plan_serializes_only_the_safe_core_contract() {
    let plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Claude,
        DesktopTransport::AnthropicBridge,
    );
    let serialized = serde_json::to_value(&plan).expect("desktop plan should serialize");

    assert_eq!(serialized["schemaVersion"], Value::from(1));
    assert_eq!(serialized["harness"], "claude-desktop");
    assert_eq!(serialized["experimental"], Value::Bool(true));
    assert_eq!(serialized["platform"], std::env::consts::OS);
    assert_eq!(serialized["transport"], "anthropic-bridge");
    assert_eq!(serialized["webSearchPolicy"], "auto");
    assert_eq!(serialized["persistentProfile"], Value::Bool(false));
    assert_eq!(serialized["privateDiagnostics"], Value::Bool(false));
    assert_eq!(serialized["restoreOnly"], Value::Bool(false));
    assert!(serialized.get("executable").is_none());
    assert!(serialized.get("selectedModel").is_none());
    assert!(serialized.get("auxiliaryModel").is_none());
    assert!(serialized.get("nativeArguments").is_none());

    assert_eq!(
        serde_json::from_value::<DesktopLaunchPlan>(serialized)
            .expect("serialized defaults should deserialize"),
        plan
    );
}

#[test]
fn explicit_desktop_launch_plan_fields_round_trip_without_normalization_loss() {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::ChatGpt,
        DesktopTransport::ResponsesBridge,
    );
    plan.executable = Some(PathBuf::from("synthetic-chatgpt"));
    plan.selected_model = Some("selected-model".to_owned());
    plan.auxiliary_model = Some("auxiliary-model".to_owned());
    plan.native_arguments = vec!["--synthetic".to_owned()];

    let serialized = serde_json::to_value(&plan).expect("desktop plan should serialize");
    assert_eq!(serialized["executable"], "synthetic-chatgpt");
    assert_eq!(serialized["selectedModel"], "selected-model");
    assert_eq!(serialized["auxiliaryModel"], "auxiliary-model");
    assert_eq!(
        serialized["nativeArguments"],
        serde_json::json!(["--synthetic"])
    );
    assert_eq!(
        serde_json::from_value::<DesktopLaunchPlan>(serialized)
            .expect("explicit plan should deserialize"),
        plan
    );
}
