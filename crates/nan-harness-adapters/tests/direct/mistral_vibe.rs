use super::support::{
    assert_direct_secret, context, plan, with_search_block, without_search_block,
};
use nan_harness_adapters::MistralVibeAdapter;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, MISTRAL_VIBE_MODEL_CATALOG_PLACEHOLDER, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, VIBE_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, PlanError, build_validated_plan};

fn config_template() -> String {
    let plan = plan(
        &MistralVibeAdapter,
        &context(HarnessKind::MistralVibe, Vec::new()),
    );
    plan.configuration_overlays[0].files[0]
        .content_template
        .clone()
}

fn parse_without_catalog(template: &str) -> toml::Table {
    toml::from_str(&template.replace(MISTRAL_VIBE_MODEL_CATALOG_PLACEHOLDER, ""))
        .expect("rendered Vibe configuration should be TOML")
}

#[test]
fn vibe_runs_with_a_launch_scoped_home_overlaid_on_the_users_home() {
    let plan = plan(
        &MistralVibeAdapter,
        &context(
            HarnessKind::MistralVibe,
            vec![
                "--trust".to_owned(),
                "-p".to_owned(),
                "inspect the project".to_owned(),
                "--output".to_owned(),
                "json".to_owned(),
            ],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        ["--trust", "-p", "inspect the project", "--output", "json"]
    );
    assert_eq!(
        plan.environment.public.get("VIBE_HOME"),
        Some(&"{artifact:vibe-home}".to_owned())
    );
    assert!(plan.temporary_artifacts.is_empty());
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Vibe home overlay should exist");
    assert_eq!(overlay.id, "vibe-home");
    assert_eq!(overlay.source_path, VIBE_HOME_PLACEHOLDER);
    let [config] = overlay.files.as_slice() else {
        panic!("Vibe should overlay only its configuration");
    };
    assert_eq!(config.path, "config.toml");
    assert_eq!(config.policy, OverlayFilePolicy::MergeTomlMcpServers);
    assert_direct_secret(&plan, "NAN_VIBE_API_KEY");
}

#[test]
fn vibe_configuration_routes_through_the_nan_provider_without_vendor_side_effects() {
    let template = config_template();
    let config = parse_without_catalog(&without_search_block(&template));

    assert_eq!(config["active_model"].as_str(), Some("qwen3.6"));
    for disabled in [
        "enable_telemetry",
        "enable_update_checks",
        "enable_auto_update",
        "experimental_enable_model_catalog",
    ] {
        assert_eq!(config[disabled].as_bool(), Some(false), "{disabled}");
    }
    let [provider] = config["providers"]
        .as_array()
        .expect("providers")
        .as_slice()
    else {
        panic!("only the NaN provider should be declared");
    };
    assert_eq!(provider["name"].as_str(), Some("nan"));
    assert_eq!(
        provider["api_base"].as_str(),
        Some(PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert_eq!(
        provider["api_key_env_var"].as_str(),
        Some("NAN_VIBE_API_KEY")
    );
    // "reasoning" would drop reasoning_content from tool-call history.
    assert_eq!(provider["api_style"].as_str(), Some("openai"));
    assert_eq!(provider["backend"].as_str(), Some("generic"));
    assert_eq!(
        provider["reasoning_field_name"].as_str(),
        Some("reasoning_content")
    );
    assert!(!config.contains_key("mcp_servers"));
    assert!(template.contains(MISTRAL_VIBE_MODEL_CATALOG_PLACEHOLDER));
}

#[test]
fn vibe_search_is_a_stdio_mcp_server_that_receives_its_token_explicitly() {
    let template = config_template();
    let table = parse_without_catalog(&with_search_block(&template));
    let [server] = table["mcp_servers"]
        .as_array()
        .expect("search server")
        .as_slice()
    else {
        panic!("only the NaN search server should be declared");
    };

    assert_eq!(server["name"].as_str(), Some("nan-search"));
    assert_eq!(server["transport"].as_str(), Some("stdio"));
    assert_eq!(server["command"].as_str(), Some("nan-harness"));
    let args = server["args"].as_array().expect("args");
    assert_eq!(args[0].as_str(), Some("__search-mcp"));
    assert_eq!(args[1].as_str(), Some("--endpoint"));
    assert!(
        args[2]
            .as_str()
            .expect("endpoint")
            .starts_with(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert_eq!(args[3].as_str(), Some("--token-env"));
    assert_eq!(args[4].as_str(), Some("NAN_VIBE_API_KEY"));
    // Vibe starts MCP servers with a reduced environment, so the token travels
    // as a runtime secret placeholder rather than inherited process state.
    assert_eq!(
        server["env"]["NAN_VIBE_API_KEY"].as_str(),
        Some("{secret:nan_api_key}")
    );
}

#[test]
fn vibe_removes_environment_that_could_override_nan_routing() {
    let plan = plan(
        &MistralVibeAdapter,
        &context(HarnessKind::MistralVibe, Vec::new()),
    );
    for name in [
        "MISTRAL_API_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "VIBE_ACTIVE_MODEL",
        "VIBE_PROVIDERS",
        "VIBE_MODELS",
        "VIBE_MCP_SERVERS",
        "VIBE_ENABLE_TELEMETRY",
        "VIBE_ENABLE_UPDATE_CHECKS",
        "VIBE_ENABLE_AUTO_UPDATE",
        "VIBE_EXPERIMENTAL_ENABLE_MODEL_CATALOG",
    ] {
        assert!(plan.environment.remove.contains(name), "{name}");
        assert!(
            plan.observability.redact_environment_names.contains(name),
            "{name}"
        );
    }
    assert!(!plan.environment.remove.contains("VIBE_HOME"));
}

#[test]
fn vibe_rejects_a_user_model_but_preserves_other_arguments() {
    for argument in ["--model", "--model=other"] {
        let error = build_validated_plan(
            &MistralVibeAdapter,
            &context(
                HarnessKind::MistralVibe,
                vec!["-p".to_owned(), "hi".to_owned(), argument.to_owned()],
            ),
        )
        .expect_err("a user model must not bypass NaN routing");
        assert!(matches!(
            error,
            PlanError::InvalidField {
                field: "process.arguments",
                ..
            }
        ));
    }
    let plan = plan(
        &MistralVibeAdapter,
        &context(
            HarnessKind::MistralVibe,
            vec![
                "--thinking".to_owned(),
                "high".to_owned(),
                "--max-turns".to_owned(),
                "3".to_owned(),
            ],
        ),
    );
    assert_eq!(
        plan.process.arguments,
        ["--thinking", "high", "--max-turns", "3"]
    );
    assert_eq!(MistralVibeAdapter.kind(), HarnessKind::MistralVibe);
}

#[test]
fn vibe_uses_its_native_compaction_threshold_and_rejects_invalid_limits() {
    use nan_harness_core::{ContextLimit, NativeContextLimit};

    let mut with_limit = context(HarnessKind::MistralVibe, Vec::new());
    let limit =
        ContextLimit::for_harness(HarnessKind::MistralVibe, 64_000, 128_000).expect("valid limit");
    assert_eq!(
        limit.native,
        NativeContextLimit::VibeThreshold {
            threshold_tokens: 64_000
        }
    );
    with_limit.context_limit = Some(limit);
    let plan = plan(&MistralVibeAdapter, &with_limit);
    let template = plan.configuration_overlays[0].files[0]
        .content_template
        .clone();
    let config = parse_without_catalog(&without_search_block(&template));
    assert_eq!(config["auto_compact_threshold"].as_integer(), Some(64_000));
    // Without a request Vibe keeps its own per-window default.
    assert!(!config_template().contains("auto_compact_threshold"));

    for requested in [0, 128_000, 200_000] {
        assert!(ContextLimit::for_harness(HarnessKind::MistralVibe, requested, 128_000).is_err());
    }
}
