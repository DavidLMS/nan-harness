use super::support::{assert_direct_secret, context, plan, without_search_block};
use nan_harness_adapters::MimoCodeAdapter;
use nan_harness_core::{ContextLimit, HarnessKind, build_validated_plan};
use serde_json::{Value, json};

#[test]
fn mimo_routes_primary_and_auxiliary_models_without_replacing_user_state() {
    let plan = plan(
        &MimoCodeAdapter,
        &context(HarnessKind::MimoCode, vec!["run".into(), "hello".into()]),
    );
    let template = &plan.environment.public["MIMOCODE_CONFIG_CONTENT"];
    let config: Value =
        serde_json::from_str(&without_search_block(template)).expect("valid overlay");
    assert_eq!(
        plan.process.arguments,
        ["--model", "nan/qwen3.6", "run", "hello"]
    );
    assert_eq!(config["enabled_providers"], json!(["nan"]));
    for field in ["model", "small_model", "vision_model"] {
        assert_eq!(config[field], "nan/qwen3.6");
    }
    for tier in ["ultra", "standard", "lite"] {
        assert_eq!(config["model_groups"][tier], "nan/qwen3.6");
    }
    assert_eq!(
        config["provider"]["nan"]["options"]["apiKey"],
        "{env:NAN_API_KEY}"
    );
    assert_eq!(config["provider"]["nan"]["only_configured_models"], true);
    assert!(template.contains("nan-search"));
    assert!(!plan.environment.public.contains_key("MIMOCODE_HOME"));
    assert!(plan.configuration_overlays.is_empty());
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn mimo_uses_its_native_compaction_limit() {
    let mut context = context(HarnessKind::MimoCode, vec![]);
    context.context_limit = Some(
        ContextLimit::for_harness(HarnessKind::MimoCode, 64_000, 128_000).expect("valid limit"),
    );
    let plan = plan(&MimoCodeAdapter, &context);
    let config: Value = serde_json::from_str(&without_search_block(
        &plan.environment.public["MIMOCODE_CONFIG_CONTENT"],
    ))
    .expect("valid overlay");
    assert_eq!(config["compaction"]["max_context"], 64_000);
    assert!(config["compaction"].get("buffer").is_none());
}

#[test]
fn mimo_rejects_model_and_remote_server_overrides() {
    for arguments in [
        vec!["--model", "other/model"],
        vec!["-m", "other/model"],
        vec!["--model=other/model"],
        vec!["-mother/model"],
        vec!["run", "--attach=http://localhost:4096"],
    ] {
        let context = context(
            HarnessKind::MimoCode,
            arguments.into_iter().map(str::to_owned).collect(),
        );
        assert!(build_validated_plan(&MimoCodeAdapter, &context).is_err());
    }
}
