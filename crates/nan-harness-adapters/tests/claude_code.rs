use nan_harness_adapters::ClaudeCodeAdapter;
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat, Transport};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlanValidator, ModelAvailability, PlanContext,
    ProfileSource, QualificationStatus, ResolvedModel, VersionStatus,
};

#[test]
fn adapter_builds_a_safe_deterministic_bridge_plan() {
    let context = context(vec!["-p".to_owned(), "hello".to_owned()]);
    let first = ClaudeCodeAdapter.plan(&context).expect("plan should build");
    let second = ClaudeCodeAdapter
        .plan(&context)
        .expect("plan should repeat");

    assert_eq!(first, second);
    LaunchPlanValidator::validate(&first).expect("plan should validate");
    assert!(matches!(first.transport, Transport::AnthropicBridge { .. }));
    assert_eq!(first.process.arguments[3], "opus");
    assert_eq!(
        &first.process.arguments[4..],
        &["-p".to_owned(), "hello".to_owned()]
    );
    let settings = first.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("settings template should exist");
    assert!(settings.contains("{runtime:claude_available_models}"));
    assert!(settings.contains("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"));
    assert!(settings.contains("{runtime:claude_model_presentations}"));
    assert!(!settings.contains("ANTHROPIC_DEFAULT_OPUS_MODEL"));
    assert!(!settings.contains("ANTHROPIC_CUSTOM_MODEL_OPTION"));
    assert!(!settings.contains("NaN · Qwen 3.6"));
    assert!(!settings.contains("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB"));
    assert!(!settings.contains("CLAUDE_CODE_SUBAGENT_MODEL"));
    assert!(!settings.contains("CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING"));
    assert!(!settings.contains("DISABLE_INTERLEAVED_THINKING"));
    let settings: serde_json::Value =
        serde_json::from_str(settings).expect("settings template should be valid JSON");
    assert!(settings.get("permissions").is_none());
    assert!(
        !first
            .environment
            .public
            .keys()
            .any(|name| name.starts_with("ANTHROPIC_DEFAULT_")),
        "picker slots must come from live discovery, not from the launch plan"
    );
    assert!(
        !first
            .environment
            .public
            .contains_key("CLAUDE_CODE_SUBAGENT_MODEL")
    );
    assert!(
        first
            .environment
            .remove
            .contains("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB")
    );
    assert!(first.environment.remove.contains("NAN_API_KEY"));
    let serialized = serde_json::to_string(&first).expect("plan should serialize");
    assert!(serialized.contains("bridge_session_token"));
    assert!(!serialized.contains("test-provider-key"));
}

#[test]
fn adapter_defers_the_model_picker_to_live_discovery() {
    for model in ["qwen3.6", "deepseek-v4-flash", "mimo-v2.5", "gemma4"] {
        let plan = ClaudeCodeAdapter
            .plan(&context_for_model(Vec::new(), model))
            .expect("supported model should plan");
        let settings = plan.temporary_artifacts[0]
            .content_template
            .as_deref()
            .expect("settings template should exist");

        assert!(settings.contains("{runtime:claude_model_presentations}"));
        assert!(
            !settings.contains("ANTHROPIC_DEFAULT_"),
            "the plan must not hardcode picker slots for {model}"
        );
        assert!(
            plan.environment
                .public
                .keys()
                .all(|name| !name.starts_with("ANTHROPIC_DEFAULT_")),
            "the launch environment must not hardcode picker slots for {model}"
        );
    }
}

#[test]
fn adapter_preserves_supported_claude_permission_modes() {
    for mode in ["default", "acceptEdits", "plan", "auto"] {
        let arguments = vec!["--permission-mode".to_owned(), mode.to_owned()];
        let plan = ClaudeCodeAdapter
            .plan(&context(arguments.clone()))
            .expect("supported permission mode should pass through");

        assert_eq!(&plan.process.arguments[4..], arguments);
    }
}

#[test]
fn adapter_preserves_local_session_arguments() {
    for arguments in [
        vec!["--continue".to_owned()],
        vec!["-c".to_owned()],
        vec!["--resume".to_owned(), "session-name".to_owned()],
        vec!["-r".to_owned(), "session-id".to_owned()],
        vec![
            "--resume".to_owned(),
            "session-name".to_owned(),
            "--fork-session".to_owned(),
        ],
    ] {
        let plan = ClaudeCodeAdapter
            .plan(&context(arguments.clone()))
            .expect("local session arguments should pass through");

        assert_eq!(&plan.process.arguments[4..], arguments);
    }
}

#[test]
fn adapter_accepts_every_native_auto_mode_argument_for_qwen() {
    for arguments in [
        vec!["--permission-mode".to_owned(), "auto".to_owned()],
        vec!["--permission-mode=auto".to_owned()],
        vec!["--enable-auto-mode".to_owned()],
    ] {
        let plan = ClaudeCodeAdapter
            .plan(&context(arguments.clone()))
            .expect("qualified Auto mode arguments should pass through");

        assert_eq!(&plan.process.arguments[4..], arguments);
    }
}

#[test]
fn adapter_rejects_auto_mode_for_other_nan_models() {
    let error = ClaudeCodeAdapter
        .plan(&context_for_model(
            vec!["--permission-mode=auto".to_owned()],
            "mimo-v2.5",
        ))
        .expect_err("Auto mode should require Qwen");

    assert_eq!(error.code(), "NH-PLAN-001");
    assert!(
        error
            .to_string()
            .contains("Claude Code Auto mode requires the qwen3.6 model")
    );
}

#[test]
fn adapter_allows_native_auto_mode_on_newer_unverified_versions() {
    for (detected_version, version_status) in [
        ("2.1.233 (Claude Code)", VersionStatus::Supported),
        ("2.1.234 (Claude Code)", VersionStatus::NewerUntested),
    ] {
        let context = context_for_version(
            vec!["--permission-mode=auto".to_owned()],
            detected_version,
            version_status,
        );

        let plan = ClaudeCodeAdapter
            .plan(&context)
            .expect("supported versions should use forward-compatible Auto safeguards");

        assert_eq!(plan.process.arguments[3], "opus");
        assert_eq!(
            plan.environment
                .public
                .get("ANTHROPIC_MODEL")
                .map(String::as_str),
            Some("opus")
        );
        assert_eq!(
            &plan.process.arguments[4..],
            &["--permission-mode=auto".to_owned()]
        );
    }
}

#[test]
fn adapter_fails_closed_for_unsupported_or_unparseable_auto_versions() {
    for (detected_version, version_status) in [
        ("2.1.100 (Claude Code)", VersionStatus::OlderUnsupported),
        ("development build", VersionStatus::Unparseable),
    ] {
        let context = context_for_version(
            vec!["--permission-mode=auto".to_owned()],
            detected_version,
            version_status,
        );

        let error = ClaudeCodeAdapter
            .plan(&context)
            .expect_err("unsupported Auto versions should fail closed");

        assert_eq!(error.code(), "NH-PLAN-001");
        assert!(
            error.to_string().contains(&format!(
                "Claude Code Auto mode requires a supported, parseable Claude Code version; detected '{detected_version}'"
            )),
            "{error}"
        );
    }
}

#[test]
fn adapter_keeps_regular_qwen_routing_for_unsupported_versions() {
    let context = context_for_version(
        Vec::new(),
        "2.1.100 (Claude Code)",
        VersionStatus::OlderUnsupported,
    );

    let plan = ClaudeCodeAdapter
        .plan(&context)
        .expect("explicitly allowed old versions should retain normal launches");

    assert_eq!(plan.process.arguments[3], "anthropic/nan/qwen3.6");
}

#[test]
fn adapter_rejects_arguments_that_can_replace_routing() {
    for argument in ["--model", "--settings=/tmp/other.json", "--teleport"] {
        let error = ClaudeCodeAdapter
            .plan(&context(vec![argument.to_owned()]))
            .expect_err("reserved argument should fail");
        assert_eq!(error.code(), "NH-PLAN-001");
    }
}

fn context(user_arguments: Vec<String>) -> PlanContext {
    context_for_model(user_arguments, "qwen3.6")
}

fn context_for_model(user_arguments: Vec<String>, model: &str) -> PlanContext {
    PlanContext {
        launch_id: LaunchId::new("launch_03claudecode").expect("valid launch ID"),
        harness: nan_harness_core::DetectedHarness {
            kind: HarnessKind::ClaudeCode,
            executable: "/usr/local/bin/claude".to_owned(),
            detected_version: "2.1.233 (Claude Code)".to_owned(),
            version_status: VersionStatus::Tested,
        },
        model: ResolvedModel {
            requested_id: model.to_owned(),
            resolved_id: model.to_owned(),
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

fn context_for_version(
    user_arguments: Vec<String>,
    detected_version: &str,
    version_status: VersionStatus,
) -> PlanContext {
    let mut context = context(user_arguments);
    detected_version.clone_into(&mut context.harness.detected_version);
    context.harness.version_status = version_status;
    context
}
