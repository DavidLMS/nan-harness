use super::support::{assert_direct_secret, assert_search_mcp, context, plan};
use nan_harness_adapters::QwenCodeAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    PROVIDER_BASE_URL_PLACEHOLDER, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
};

#[test]
fn qwen_code_uses_openai_environment_routing_without_hiding_customizations() {
    let plan = plan(
        &QwenCodeAdapter,
        &context(
            HarnessKind::QwenCode,
            vec!["--prompt".to_owned(), "inspect the project".to_owned()],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        ["--model", "qwen3.6", "--prompt", "inspect the project"]
    );
    assert_eq!(
        plan.environment.public.get("OPENAI_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("OPENAI_MODEL"),
        Some(&"qwen3.6".to_owned())
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Qwen Code settings overlay should exist");
    let settings: serde_json::Value = serde_json::from_str(&overlay.files[0].content_template)
        .expect("Qwen Code settings should be JSON");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.qwen");
    assert_eq!(overlay.files[0].path, "settings.json");
    assert_eq!(
        settings["modelProviders"]["openai"],
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER
    );
    assert_eq!(settings["tools"]["listDirectory"]["enabled"], true);
    let search_file = overlay
        .files
        .iter()
        .find(|file| file.path == "mcp.json")
        .expect("Qwen Code search MCP settings should exist");
    assert_search_mcp(&search_file.content_template, "OPENAI_API_KEY");
    assert_eq!(
        plan.environment.public.get("QWEN_HOME"),
        Some(&"{artifact:qwen-config}".to_owned())
    );
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}
