use super::support::{assert_direct_secret, context, plan, without_search_block};
use nan_harness_adapters::OpenCodeAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
};

#[test]
fn opencode_uses_an_inline_provider_overlay_without_hiding_user_plugins() {
    let plan = plan(
        &OpenCodeAdapter,
        &context(HarnessKind::OpenCode, Vec::new()),
    );
    let template = plan
        .environment
        .public
        .get("OPENCODE_CONFIG_CONTENT")
        .expect("OpenCode overlay should exist");
    let config: serde_json::Value = serde_json::from_str(&without_search_block(template))
        .expect("OpenCode overlay without its conditional block should be JSON");

    assert_eq!(plan.process.arguments, ["--model", "nan/qwen3.6"]);
    assert_eq!(config["enabled_providers"], serde_json::json!(["nan"]));
    assert_eq!(
        config["provider"]["nan"]["options"]["apiKey"],
        "{env:NAN_API_KEY}"
    );
    assert!(template.contains("\"nan-search\""));
    assert!(template.contains("\"__search-mcp\""));
    assert!(template.contains(BRIDGE_BASE_URL_PLACEHOLDER));
    assert_eq!(
        config["provider"]["nan"]["models"],
        OPENCODE_MODEL_CATALOG_PLACEHOLDER
    );
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn opencode_v2_uses_private_server_and_native_provider_schema() {
    let mut context = context(HarnessKind::OpenCode, vec![]);
    context.harness.detected_version = "opencode v2.0.27".to_owned();
    let plan = plan(&OpenCodeAdapter, &context);
    let template = &plan.environment.public["OPENCODE_CONFIG_CONTENT"];
    let config: serde_json::Value = serde_json::from_str(&without_search_block(template)).unwrap();
    assert_eq!(plan.process.arguments, ["--standalone"]);
    assert_eq!(config["model"], "nan/qwen3.6");
    assert_eq!(
        config["providers"]["nan"]["package"],
        "@opencode/ai/providers/openai-compatible"
    );
    assert_eq!(
        config["providers"]["nan"]["settings"]["apiKey"],
        "{env:NAN_API_KEY}"
    );
    assert_eq!(
        config["providers"]["nan"]["models"],
        nan_harness_core::launch_plan::OPENCODE_V2_MODEL_CATALOG_PLACEHOLDER
    );
    let full = template
        .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_BEGIN, "")
        .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_END, "");
    let full: serde_json::Value = serde_json::from_str(&full).unwrap();
    assert_eq!(full["mcp"]["servers"]["nan-search"]["disabled"], false);
    assert!(config.get("plugins").is_none());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn opencode_v2_places_run_flags_after_subcommand() {
    for command in ["run", "mini"] {
        let mut context = context(
            HarnessKind::OpenCode,
            vec![command.to_owned(), "synthetic prompt".to_owned()],
        );
        context.harness.detected_version = "2.0.27".to_owned();
        assert_eq!(
            plan(&OpenCodeAdapter, &context).process.arguments,
            [
                command,
                "--standalone",
                "--model",
                "nan/qwen3.6",
                "synthetic prompt"
            ]
        );
    }
}

#[test]
fn opencode_v2_rejects_arguments_that_bypass_managed_routing() {
    use nan_harness_core::HarnessAdapter;
    for argument in [
        "--server=http://localhost:1234",
        "--standalone=false",
        "--pure",
        "--variant=max",
        "--model=other/model",
        "-mother/model",
    ] {
        let mut context = context(
            HarnessKind::OpenCode,
            vec!["run".to_owned(), argument.to_owned()],
        );
        context.harness.detected_version = "v2.0.27".to_owned();
        assert!(
            OpenCodeAdapter.plan(&context).is_err(),
            "accepted {argument}"
        );
    }
}

#[test]
fn opencode_v2_preserves_the_native_compaction_budget() {
    let mut context = context(HarnessKind::OpenCode, Vec::new());
    context.harness.detected_version = "2.0.27".to_owned();
    context.context_limit = Some(nan_harness_core::ContextLimit {
        requested_tokens: 80000,
        effective_context_window: 100_000,
        native: nan_harness_core::NativeContextLimit::OpenCodeBuffer {
            buffer_tokens: 20000,
        },
    });
    let plan = plan(&OpenCodeAdapter, &context);
    let config: serde_json::Value = serde_json::from_str(&without_search_block(
        &plan.environment.public["OPENCODE_CONFIG_CONTENT"],
    ))
    .unwrap();
    assert_eq!(
        config["compaction"],
        serde_json::json!({"auto": true, "buffer": 20000})
    );
}
