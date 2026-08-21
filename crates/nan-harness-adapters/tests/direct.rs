use nan_harness_adapters::{
    AiderAdapter, ClineAdapter, CodexAdapter, DeepSeekHarnessAdapter, GooseAdapter, HermesAdapter,
    KimiCodeAdapter, OpenClawAdapter, OpenCodeAdapter, PersistentAiderAdapter,
    PersistentDeepSeekHarnessAdapter, PersistentPiAdapter, PersistentPrimeAgentAdapter,
    PersistentQwenCodeAdapter, PiAdapter, PrimeAgentAdapter, QwenCodeAdapter,
    persistent_provider_extension,
};
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    BRIDGE_BASE_URL_PLACEHOLDER, CLINE_MODEL_CATALOG_PLACEHOLDER, CODEX_HOME_ARTIFACT_PLACEHOLDER,
    CODEX_HOME_OVERLAY_ID, CODEX_PROFILE_ARTIFACT_ID, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    GOOSE_MODEL_CATALOG_PLACEHOLDER, HERMES_MODEL_CATALOG_PLACEHOLDER,
    KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, LaunchId, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER, ObservabilityFormat,
    OverlayFilePolicy, PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER, Protocol,
    QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
    Transport,
};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{
    DetectedHarness, HarnessAdapter, HarnessCapability, HarnessKind, PlanContext, PlanError,
    ResolvedModel, VersionStatus, build_validated_plan,
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
        config["provider"]["nan"]["models"],
        OPENCODE_MODEL_CATALOG_PLACEHOLDER
    );
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn codex_uses_a_launch_scoped_profile_without_replacing_user_state() {
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
    assert_eq!(
        &plan.process.arguments[..2],
        ["--profile", "nan-harness-launch_01directadapter"]
    );
    assert!(
        plan.process
            .arguments
            .iter()
            .any(|argument| argument == "model=\"qwen3.6\"")
    );
    assert!(plan.process.arguments.iter().any(|argument| {
        argument
            == &format!("model_reasoning_effort=\"{SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER}\"")
    }));
    assert!(
        plan.process
            .arguments
            .contains(&"features.standalone_web_search=true".to_owned())
    );
    assert!(
        plan.process
            .arguments
            .windows(2)
            .any(|arguments| arguments == ["--disable", "apps"])
    );
    assert_eq!(plan.temporary_artifacts.len(), 1);
    assert!(!plan.environment.public.contains_key("CODEX_HOME"));
    assert!(plan.configuration_overlays.is_empty());
    assert_eq!(plan.launch_scoped_files.len(), 1);
    assert_eq!(plan.launch_scoped_files[0].id, CODEX_PROFILE_ARTIFACT_ID);
    assert_eq!(
        plan.launch_scoped_files[0].directory,
        "{runtime:codex_home}"
    );
    assert_eq!(
        plan.launch_scoped_files[0].file_name,
        "nan-harness-launch_01directadapter.config.toml"
    );
    assert!(
        plan.launch_scoped_files[0]
            .content_template
            .contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER)
    );
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
fn codex_without_profile_support_uses_the_legacy_isolated_home() {
    let mut context = context(HarnessKind::Codex, Vec::new());
    context.harness.capabilities.clear();
    let plan = plan(&CodexAdapter, &context);

    assert_eq!(
        plan.environment
            .public
            .get("CODEX_HOME")
            .map(String::as_str),
        Some(CODEX_HOME_ARTIFACT_PLACEHOLDER)
    );
    assert!(plan.launch_scoped_files.is_empty());
    assert_eq!(plan.configuration_overlays.len(), 1);
    assert_eq!(plan.configuration_overlays[0].id, CODEX_HOME_OVERLAY_ID);
    assert_eq!(
        plan.configuration_overlays[0].files[0].policy,
        OverlayFilePolicy::MergeToml
    );
}

#[test]
fn hermes_loads_a_launch_scoped_nan_provider_without_hiding_user_state() {
    let plan = plan(
        &HermesAdapter,
        &context(HarnessKind::Hermes, vec!["--tui".to_owned()]),
    );

    assert_eq!(
        plan.process.arguments,
        ["--provider", "nan", "--model", "qwen3.6", "--tui"]
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Hermes home overlay should exist");
    let plugin = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("__init__.py"))
        .expect("NaN provider plugin should exist");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.hermes");
    assert_eq!(
        plan.environment.public.get("HERMES_HOME"),
        Some(&"{artifact:hermes-home}".to_owned())
    );
    assert!(
        plugin
            .content_template
            .contains(PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert!(
        plugin
            .content_template
            .contains(HERMES_MODEL_CATALOG_PLACEHOLDER)
    );
    assert!(plan.environment.remove.contains("OPENAI_BASE_URL"));
    assert_direct_secret(&plan, "NAN_API_KEY");
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
        assert!(extension.contains(PROVIDER_BASE_URL_PLACEHOLDER));
        assert!(extension.contains("const apiKey = process.env.NAN_API_KEY"));
        assert!(extension.contains(PI_MODEL_CATALOG_PLACEHOLDER));
        assert!(extension.contains("profile.reasoningPolicy.kind"));
        assert!(extension.contains("thinkingLevelMap"));
        assert!(!extension.contains("reasoning: false"));
        assert!(!extension.contains("fetch(`${baseUrl}/models`"));
        assert_direct_secret(&plan, "NAN_API_KEY");
    }
}

#[test]
fn persistent_pi_adapters_reuse_the_global_provider_without_a_temporary_extension() {
    for (adapter, kind) in [
        (&PersistentPiAdapter as &dyn HarnessAdapter, HarnessKind::Pi),
        (
            &PersistentPrimeAgentAdapter as &dyn HarnessAdapter,
            HarnessKind::PrimeAgent,
        ),
    ] {
        let plan = plan(adapter, &context(kind, vec!["--continue".to_owned()]));

        assert_eq!(
            plan.process.arguments,
            [
                "--provider",
                "nan",
                "--model",
                "qwen3.6",
                "--models",
                "nan/*",
                "--continue"
            ]
        );
        assert!(plan.temporary_artifacts.is_empty());
        assert_direct_secret(&plan, "NAN_API_KEY");
    }
}

#[test]
fn persistent_pi_discovery_keeps_reasoning_model_aware_for_known_and_generic_models() {
    let extension = persistent_provider_extension("https://nan.invalid/v1")
        .expect("persistent Pi extension should render");

    assert!(extension.contains("reasoningPolicy"));
    assert!(extension.contains("thinkingLevelMap"));
    assert!(extension.contains(r#"reasoningPolicy: { kind: "unknown" }"#));
    assert!(!extension.contains("reasoning: false"));
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
    assert!(patch.contains(DEEPSEEK_MODEL_CATALOG_PLACEHOLDER));
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
fn persistent_deepseek_harness_only_overlays_the_selected_model() {
    let plan = plan(
        &PersistentDeepSeekHarnessAdapter,
        &context(HarnessKind::DeepSeekHarness, Vec::new()),
    );
    let patch = plan.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("model selector should have content");

    assert!(patch.contains("provider: nan-harness"));
    assert!(patch.contains("model: \"qwen3.6\""));
    assert!(!patch.contains("llm-pi-ai"));
    assert!(!patch.contains(DEEPSEEK_MODEL_CATALOG_PLACEHOLDER));
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn openclaw_merges_user_configuration_without_persisting_the_nan_secret() {
    let plan = plan(
        &OpenClawAdapter,
        &context(HarnessKind::OpenClaw, Vec::new()),
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("OpenClaw overlay should exist");
    let config: serde_json::Value = serde_json::from_str(
        &overlay
            .files
            .iter()
            .find(|file| file.path == "nan-harness.json")
            .expect("NaN configuration should exist")
            .content_template,
    )
    .expect("OpenClaw configuration should be JSON");

    assert_eq!(plan.process.arguments, ["chat"]);
    assert_eq!(overlay.source_path, "{runtime:user_home}/.openclaw");
    assert_eq!(config["$include"], "./openclaw.json");
    assert_eq!(
        config["models"]["providers"]["nan"]["apiKey"],
        serde_json::json!({
            "id": "NAN_API_KEY",
            "provider": "default",
            "source": "env"
        })
    );
    assert_eq!(
        config["agents"]["defaults"]["models"],
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER
    );
    assert_eq!(
        config["models"]["providers"]["nan"]["models"],
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER
    );
    assert!(
        !overlay
            .files
            .iter()
            .any(|file| file.content_template.contains("nan-secret-value"))
    );
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn cline_merges_provider_routing_and_models_into_linked_user_settings() {
    let plan = plan(&ClineAdapter, &context(HarnessKind::Cline, Vec::new()));
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Cline overlay should exist");
    let provider_file = overlay
        .files
        .iter()
        .find(|file| file.path == "data/settings/providers.json")
        .expect("Cline provider settings should exist");
    let models_file = overlay
        .files
        .iter()
        .find(|file| file.path == "data/settings/models.json")
        .expect("Cline model catalog should exist");
    let settings: serde_json::Value = serde_json::from_str(&provider_file.content_template)
        .expect("Cline settings should be JSON");

    assert_eq!(overlay.source_path, "{runtime:user_home}/.cline");
    assert_eq!(
        plan.process.arguments,
        [
            "--config",
            "{artifact:cline-config}",
            "--provider",
            "openai-compatible",
            "--model",
            "qwen3.6"
        ]
    );
    assert_eq!(
        settings["providers"]["openai-compatible"]["settings"]["baseUrl"],
        PROVIDER_BASE_URL_PLACEHOLDER
    );
    assert!(
        settings["providers"]["openai-compatible"]["settings"]
            .get("apiKey")
            .is_none()
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&models_file.content_template)
            .expect("Cline model catalog should be JSON")["providers"]["openai-compatible"]["models"],
        CLINE_MODEL_CATALOG_PLACEHOLDER
    );
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}

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
    assert_eq!(
        plan.environment.public.get("QWEN_HOME"),
        Some(&"{artifact:qwen-config}".to_owned())
    );
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}

#[test]
fn persistent_qwen_code_uses_the_user_catalog_without_a_temporary_home() {
    let plan = plan(
        &PersistentQwenCodeAdapter,
        &context(HarnessKind::QwenCode, Vec::new()),
    );

    assert_eq!(
        plan.process.arguments,
        ["--auth-type", "openai", "--model", "qwen3.6"]
    );
    assert!(plan.configuration_overlays.is_empty());
    assert!(plan.temporary_artifacts.is_empty());
    assert!(!plan.environment.public.contains_key("QWEN_HOME"));
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn kimi_code_exposes_a_launch_scoped_model_catalog() {
    let plan = plan(
        &KimiCodeAdapter,
        &context(
            HarnessKind::KimiCode,
            vec!["--prompt".to_owned(), "inspect the project".to_owned()],
        ),
    );

    assert_eq!(plan.process.arguments, ["--prompt", "inspect the project"]);
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_PROVIDER_TYPE"),
        Some(&"openai".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_DISPLAY_NAME"),
        Some(&SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_MAX_CONTEXT_SIZE"),
        Some(&SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_MAX_OUTPUT_SIZE"),
        Some(&SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_CAPABILITIES"),
        Some(&SELECTED_MODEL_CAPABILITIES_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_CODE_HOME"),
        Some(&"{artifact:kimi-code-home}".to_owned())
    );
    assert!(plan.temporary_artifacts.is_empty());
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Kimi Code home overlay should exist");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.kimi-code");
    let config = overlay
        .files
        .first()
        .expect("Kimi Code config overlay should exist");
    assert_eq!(config.path, "config.toml");
    assert_eq!(config.content_template, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER);
    assert_eq!(config.policy, OverlayFilePolicy::MergeToml);
    assert_direct_secret(&plan, "KIMI_MODEL_API_KEY");
}

#[test]
fn aider_pins_every_internal_model_without_replacing_user_state() {
    let plan = plan(
        &AiderAdapter,
        &context(
            HarnessKind::Aider,
            vec!["--message".to_owned(), "inspect the project".to_owned()],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        [
            "--model",
            "openai/qwen3.6",
            "--weak-model",
            "openai/qwen3.6",
            "--editor-model",
            "openai/qwen3.6",
            "--model-settings-file",
            "{artifact:aider-model-settings}",
            "--model-metadata-file",
            "{artifact:aider-model-metadata}",
            "--message",
            "inspect the project"
        ]
    );
    assert_eq!(
        plan.environment.public.get("AIDER_OPENAI_API_BASE"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(plan.temporary_artifacts.len(), 2);
    assert_eq!(
        plan.temporary_artifacts[0].content_template.as_deref(),
        Some(AIDER_MODEL_SETTINGS_PLACEHOLDER)
    );
    assert_eq!(
        plan.temporary_artifacts[1].content_template.as_deref(),
        Some(AIDER_MODEL_METADATA_PLACEHOLDER)
    );
    assert_direct_secret(&plan, "AIDER_OPENAI_API_KEY");
}

#[test]
fn persistent_aider_uses_nan_aliases_from_user_model_files() {
    let plan = plan(
        &PersistentAiderAdapter,
        &context(HarnessKind::Aider, Vec::new()),
    );

    assert_eq!(
        plan.process.arguments,
        [
            "--model",
            "nan/qwen3.6",
            "--weak-model",
            "nan/qwen3.6",
            "--editor-model",
            "nan/qwen3.6"
        ]
    );
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn goose_routes_with_environment_without_hiding_user_extensions() {
    let plan = plan(
        &GooseAdapter,
        &context(
            HarnessKind::Goose,
            vec!["run".to_owned(), "--text".to_owned(), "inspect".to_owned()],
        ),
    );

    assert_eq!(plan.process.arguments, ["run", "--text", "inspect"]);
    assert_eq!(
        plan.environment.public.get("OPENAI_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_PROVIDER"),
        Some(&"openai".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_MODEL"),
        Some(&"qwen3.6".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_PREDEFINED_MODELS"),
        Some(&GOOSE_MODEL_CATALOG_PLACEHOLDER.to_owned())
    );
    assert!(plan.configuration_overlays.is_empty());
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}

#[test]
fn goose_defaults_to_an_interactive_session() {
    let plan = plan(&GooseAdapter, &context(HarnessKind::Goose, Vec::new()));

    assert_eq!(plan.process.arguments, ["session"]);
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
        (
            &OpenClawAdapter as &dyn HarnessAdapter,
            HarnessKind::OpenClaw,
            "--model=other/model",
        ),
        (
            &ClineAdapter as &dyn HarnessAdapter,
            HarnessKind::Cline,
            "--config=other",
        ),
        (
            &QwenCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::QwenCode,
            "--fallback-model=other",
        ),
        (
            &KimiCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::KimiCode,
            "--model=other",
        ),
        (
            &AiderAdapter as &dyn HarnessAdapter,
            HarnessKind::Aider,
            "--weak-model=other",
        ),
        (
            &GooseAdapter as &dyn HarnessAdapter,
            HarnessKind::Goose,
            "--model=other",
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
            capabilities: (kind == HarnessKind::Codex)
                .then_some(HarnessCapability::CodexConfigProfile)
                .into_iter()
                .collect(),
        },
        model: ResolvedModel {
            requested_id: "qwen3.6".to_owned(),
            resolved_id: "qwen3.6".to_owned(),
            reasoning_selection: None,
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
