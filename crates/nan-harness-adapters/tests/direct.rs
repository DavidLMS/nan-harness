use nan_harness_adapters::{
    CodexAdapter, DeepSeekHarnessAdapter, HermesAdapter, OpenCodeAdapter, PiAdapter,
    PrimeAgentAdapter,
};
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, LaunchId, ObservabilityFormat, PROVIDER_BASE_URL_PLACEHOLDER,
    Protocol, Transport,
};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{
    DetectedHarness, HarnessAdapter, HarnessKind, PlanContext, PlanError, ResolvedModel,
    VersionStatus, build_validated_plan,
};

#[test]
fn opencode_uses_an_inline_provider_overlay_without_hiding_user_plugins() {
    let plan = plan(
        &OpenCodeAdapter,
        &context(HarnessKind::OpenCode, Vec::new()),
    );
    let config: serde_json::Value = serde_json::from_str(
        plan.environment
            .public
            .get("OPENCODE_CONFIG_CONTENT")
            .expect("OpenCode overlay should exist"),
    )
    .expect("OpenCode overlay should be JSON");

    assert_eq!(plan.process.arguments, ["--model", "nan/qwen3.6"]);
    assert_eq!(config["enabled_providers"], serde_json::json!(["nan"]));
    assert_eq!(
        config["provider"]["nan"]["options"]["apiKey"],
        "{env:NAN_API_KEY}"
    );
    assert_eq!(
        config["provider"]["nan"]["models"]["qwen3.6"]["name"],
        "NaN · Qwen 3.6"
    );
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn codex_uses_temporary_config_overrides_without_replacing_user_state() {
    let plan = plan(&CodexAdapter, &context(HarnessKind::Codex, Vec::new()));

    assert!(matches!(
        &plan.transport,
        Transport::ResponsesBridge {
            client_protocol: Protocol::OpenAiResponses,
            upstream_protocol: Protocol::ChatCompletions,
            ..
        }
    ));
    assert!(
        plan.process
            .arguments
            .iter()
            .any(|argument| argument.contains(&format!("{BRIDGE_BASE_URL_PLACEHOLDER}/v1")))
    );
    assert!(
        plan.process
            .arguments
            .windows(2)
            .any(|arguments| { arguments == ["--model".to_owned(), "qwen3.6".to_owned()] })
    );
    assert!(
        plan.process
            .arguments
            .contains(&"features.standalone_web_search=true".to_owned())
    );
    assert_eq!(plan.temporary_artifacts.len(), 1);
    assert_eq!(
        plan.temporary_artifacts[0].content_template.as_deref(),
        Some("{runtime:codex_model_catalog}")
    );
    assert!(
        plan.process.arguments.iter().any(|argument| {
            argument == "model_catalog_json=\"{artifact:codex-model-catalog}\""
        })
    );
    assert_eq!(
        plan.environment
            .secrets
            .get("NAN_HARNESS_SESSION_TOKEN")
            .expect("session token should be injected")
            .as_str(),
        "bridge_session_token"
    );
}

#[test]
fn hermes_selects_its_launch_scoped_custom_provider() {
    let plan = plan(
        &HermesAdapter,
        &context(HarnessKind::Hermes, vec!["--tui".to_owned()]),
    );

    assert_eq!(
        plan.process.arguments,
        ["--provider", "custom", "--model", "qwen3.6", "--tui"]
    );
    assert_eq!(
        plan.environment.public.get("CUSTOM_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert!(plan.environment.remove.contains("OPENAI_BASE_URL"));
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}

#[test]
fn pi_and_prime_agent_load_the_same_ephemeral_provider_extension() {
    for (adapter, kind) in [
        (&PiAdapter as &dyn HarnessAdapter, HarnessKind::Pi),
        (
            &PrimeAgentAdapter as &dyn HarnessAdapter,
            HarnessKind::PrimeAgent,
        ),
    ] {
        let plan = plan(adapter, &context(kind, vec!["--continue".to_owned()]));
        let extension = plan.temporary_artifacts[0]
            .content_template
            .as_deref()
            .expect("provider extension should have content");

        assert_eq!(
            plan.process.arguments,
            [
                "--extension",
                "{artifact:pi-provider-extension}",
                "--provider",
                "nan",
                "--model",
                "qwen3.6",
                "--models",
                "nan/*",
                "--continue"
            ]
        );
        assert!(extension.contains("pi.registerProvider(\"nan\""));
        assert!(extension.contains("process.env.NAN_HARNESS_PROVIDER_BASE_URL"));
        assert!(extension.contains("\"id\":\"qwen3.6\""));
        assert_direct_secret(&plan, "NAN_API_KEY");
    }
}

#[test]
fn deepseek_harness_uses_a_highest_precedence_patch_and_disables_its_telemetry() {
    let plan = plan(
        &DeepSeekHarnessAdapter,
        &context(HarnessKind::DeepSeekHarness, Vec::new()),
    );
    let patch = plan.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("DeepSeek Harness patch should have content");

    assert_eq!(
        plan.process.arguments,
        ["web", "--patch", "{artifact:deepseek-harness-patch}"]
    );
    assert_eq!(
        plan.environment.public.get("DSH_TELEMETRY_DISABLED"),
        Some(&"1".to_owned())
    );
    assert!(patch.contains("provider: nan-harness"));
    assert!(patch.contains("api: openai-completions"));
    assert!(patch.contains("baseURL: !!js process.env.NAN_HARNESS_PROVIDER_BASE_URL"));
    assert!(patch.contains("- id: web-search-deepseek\n  disabled: true"));
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn deepseek_harness_preserves_an_explicit_headless_profile() {
    let plan = plan(
        &DeepSeekHarnessAdapter,
        &context(
            HarnessKind::DeepSeekHarness,
            vec![
                "--profile".to_owned(),
                "headless".to_owned(),
                "inspect the project".to_owned(),
            ],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        [
            "--profile",
            "headless",
            "--patch",
            "{artifact:deepseek-harness-patch}",
            "inspect the project"
        ]
    );
}

#[test]
fn direct_adapters_reject_arguments_that_can_bypass_nan_routing() {
    for (adapter, kind, argument) in [
        (
            &OpenCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::OpenCode,
            "--model=other/model",
        ),
        (
            &HermesAdapter as &dyn HarnessAdapter,
            HarnessKind::Hermes,
            "--provider",
        ),
        (
            &PiAdapter as &dyn HarnessAdapter,
            HarnessKind::Pi,
            "--api-key",
        ),
        (
            &DeepSeekHarnessAdapter as &dyn HarnessAdapter,
            HarnessKind::DeepSeekHarness,
            "--patch=other.yml",
        ),
    ] {
        let error = build_validated_plan(adapter, &context(kind, vec![argument.to_owned()]))
            .expect_err("routing override should fail");
        assert!(matches!(
            error,
            PlanError::InvalidField {
                field: "process.arguments",
                ..
            }
        ));
    }
}

fn plan(adapter: &dyn HarnessAdapter, context: &PlanContext) -> nan_harness_core::LaunchPlan {
    build_validated_plan(adapter, context).expect("adapter should produce a valid plan")
}

fn context(kind: HarnessKind, user_arguments: Vec<String>) -> PlanContext {
    PlanContext {
        launch_id: LaunchId::new("launch_01directadapter").expect("valid launch ID"),
        harness: DetectedHarness {
            kind,
            executable: format!("/usr/local/bin/{}", kind.binary_name()),
            detected_version: "test-version".to_owned(),
            version_status: VersionStatus::Tested,
        },
        model: ResolvedModel {
            requested_id: "qwen3.6".to_owned(),
            resolved_id: "qwen3.6".to_owned(),
            availability: ModelAvailability::Discovered,
            profile_source: ProfileSource::Bundled,
            qualification: QualificationStatus::Qualified,
            warnings: Vec::new(),
        },
        working_directory: "/workspace/project".to_owned(),
        user_arguments,
        observability_format: ObservabilityFormat::Human,
    }
}

fn assert_direct_secret(plan: &nan_harness_core::LaunchPlan, target: &str) {
    assert!(matches!(
        &plan.transport,
        Transport::DirectChat {
            base_url,
            credential_target,
            ..
        } if base_url == PROVIDER_BASE_URL_PLACEHOLDER && credential_target == target
    ));
    assert_eq!(
        plan.environment
            .secrets
            .get(target)
            .expect("credential target should be mapped")
            .as_str(),
        "nan_api_key"
    );
    assert!(
        !serde_json::to_string(plan)
            .expect("plan should serialize")
            .contains("nan-secret-value")
    );
}
