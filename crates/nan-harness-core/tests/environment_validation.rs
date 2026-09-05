use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
    CODEX_HOME_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, LaunchPlanValidator,
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
    QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
    USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{ErrorCategory, LaunchPlan, PlanError};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");
const BRIDGE_PLAN: &str = include_str!("fixtures/launch-plan.bridge.json");

#[test]
fn environment_names_accept_valid_boundaries_and_reject_invalid_names() {
    for name in ["_", "_A", "A", "A0", "A_B"] {
        let mut plan = direct_plan();
        plan.environment
            .public
            .insert(name.to_owned(), "value".to_owned());
        LaunchPlanValidator::validate(&plan).expect("valid environment name should pass");
    }

    for name in ["", "a", "1A", "A-a", "A a", "Aé"] {
        let mut plan = direct_plan();
        plan.environment
            .public
            .insert(name.to_owned(), "value".to_owned());
        let error = LaunchPlanValidator::validate(&plan).expect_err("invalid name should fail");
        assert_invalid_field(&error, "environment");
    }
}

#[test]
fn environment_conflicts_are_rejected_for_public_secret_and_remove_pairs() {
    let pairs = [
        ("public", "secret"),
        ("public", "remove"),
        ("secret", "remove"),
    ];
    for (left, right) in pairs {
        let mut plan = direct_plan();
        match left {
            "public" => {
                plan.environment
                    .public
                    .insert("SYNTHETIC_TOKEN".to_owned(), "public".to_owned());
            }
            "secret" => {
                plan.environment.secrets.insert(
                    "SYNTHETIC_TOKEN".to_owned(),
                    nan_harness_core::SecretRef::new("synthetic_token").expect("valid reference"),
                );
                plan.observability
                    .redact_environment_names
                    .insert("SYNTHETIC_TOKEN".to_owned());
            }
            _ => unreachable!(),
        }
        match right {
            "secret" => {
                plan.environment.secrets.insert(
                    "SYNTHETIC_TOKEN".to_owned(),
                    nan_harness_core::SecretRef::new("synthetic_token").expect("valid reference"),
                );
                plan.observability
                    .redact_environment_names
                    .insert("SYNTHETIC_TOKEN".to_owned());
            }
            "remove" => {
                plan.environment.remove.insert("SYNTHETIC_TOKEN".to_owned());
            }
            _ => unreachable!(),
        }

        let error = LaunchPlanValidator::validate(&plan).expect_err("conflict should fail");
        assert_eq!(
            error,
            PlanError::ConflictingEnvironment {
                variable: "SYNTHETIC_TOKEN".to_owned(),
            }
        );
        assert_eq!(error.category(), ErrorCategory::Security);
    }
}

#[test]
fn secret_environment_names_require_redaction_and_public_values_remain_distinct() {
    let mut missing_redaction = direct_plan();
    missing_redaction
        .observability
        .redact_environment_names
        .remove("NAN_API_KEY");
    let error = LaunchPlanValidator::validate(&missing_redaction).expect_err("secret must redact");
    assert_invalid_field(&error, "observability.redactEnvironmentNames");

    let mut public_value = direct_plan();
    public_value.environment.public.insert(
        "SYNTHETIC_PUBLIC".to_owned(),
        "synthetic-public-value".to_owned(),
    );
    LaunchPlanValidator::validate(&public_value).expect("public value should remain valid");
    assert_eq!(
        public_value.environment.public["SYNTHETIC_PUBLIC"],
        "synthetic-public-value"
    );
    assert_eq!(
        public_value.environment.secrets["NAN_API_KEY"].as_str(),
        "nan_api_key"
    );
}

#[test]
fn transport_secret_references_must_map_to_secret_environment_entries() {
    let mut direct = direct_plan();
    if let nan_harness_core::launch_plan::Transport::DirectChat {
        credential_target, ..
    } = &mut direct.transport
    {
        *credential_target = "PUBLIC_ONLY".to_owned();
    }
    direct.environment.public.insert(
        "PUBLIC_ONLY".to_owned(),
        "synthetic-public-value".to_owned(),
    );
    let error = LaunchPlanValidator::validate(&direct).expect_err("public value is not a secret");
    assert_eq!(
        error,
        PlanError::MissingSecretReference {
            reference: "PUBLIC_ONLY".to_owned(),
        }
    );
    assert_eq!(error.category(), ErrorCategory::Security);

    let mut bridge = bridge_plan();
    bridge.environment.secrets.clear();
    let error = LaunchPlanValidator::validate(&bridge).expect_err("bridge token must be mapped");
    assert_eq!(
        error,
        PlanError::MissingSecretReference {
            reference: "bridge_session_token".to_owned(),
        }
    );
}

#[test]
fn supported_runtime_and_feature_placeholders_are_accepted() {
    let mut direct = direct_plan();
    direct.temporary_artifacts[0].content_template = Some(
        [
            PROVIDER_BASE_URL_PLACEHOLDER,
            CODEX_HOME_PLACEHOLDER,
            USER_HOME_PLACEHOLDER,
            CODEX_MODEL_CATALOG_PLACEHOLDER,
            SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
            SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
            SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
            SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
            SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
            CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
            CLAUDE_MODEL_PICKER_PLACEHOLDER,
            CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
            DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
            GOOSE_MODEL_CATALOG_PLACEHOLDER,
            GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER,
            HERMES_MODEL_CATALOG_PLACEHOLDER,
            OPENCODE_MODEL_CATALOG_PLACEHOLDER,
            OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
            OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
            PI_MODEL_CATALOG_PLACEHOLDER,
            QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
            KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
            "{env:NAN_API_KEY}",
        ]
        .join(" "),
    );
    LaunchPlanValidator::validate(&direct).expect("supported runtime placeholders should pass");

    let mut bridge = bridge_plan();
    bridge.temporary_artifacts[0].content_template = Some(format!(
        "{BRIDGE_BASE_URL_PLACEHOLDER} {{secret:bridge_session_token}}"
    ));
    LaunchPlanValidator::validate(&bridge).expect("mapped session token placeholder should pass");
}

#[test]
fn unknown_and_malformed_placeholders_report_the_resource_and_security_category() {
    for template in [
        "{runtime:unknown_feature}",
        "{secret:unknown_secret}",
        "{runtime:}",
        "{secret:}",
        "{runtime:unknown{secret:nested}}",
    ] {
        let mut plan = direct_plan();
        plan.temporary_artifacts[0].content_template = Some(template.to_owned());
        let error =
            LaunchPlanValidator::validate(&plan).expect_err("unknown placeholder should fail");
        assert_unsafe_resource(&error, "opencode-config");
    }
}

#[test]
fn search_blocks_accept_ordered_boundaries_and_reject_unordered_or_nested_blocks() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(format!(
        "prefix{NAN_SEARCH_BLOCK_BEGIN}synthetic-search{NAN_SEARCH_BLOCK_END}suffix"
    ));
    LaunchPlanValidator::validate(&plan).expect("ordered search block should pass");

    for template in [
        format!("{NAN_SEARCH_BLOCK_END}before-begin"),
        format!("{NAN_SEARCH_BLOCK_BEGIN}missing-end"),
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN}{NAN_SEARCH_BLOCK_BEGIN}nested{NAN_SEARCH_BLOCK_END}{NAN_SEARCH_BLOCK_END}"
        ),
    ] {
        plan.temporary_artifacts[0].content_template = Some(template);
        let error =
            LaunchPlanValidator::validate(&plan).expect_err("malformed search block should fail");
        assert_unsafe_resource(&error, "opencode-config");
    }
}

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn bridge_plan() -> LaunchPlan {
    serde_json::from_str(BRIDGE_PLAN).expect("valid bridge plan fixture")
}

fn assert_invalid_field(error: &PlanError, field: &'static str) {
    assert_eq!(error.category(), ErrorCategory::Contract);
    assert!(matches!(
        error,
        PlanError::InvalidField { field: actual, .. } if *actual == field
    ));
}

fn assert_unsafe_resource(error: &PlanError, artifact_id: &str) {
    assert_eq!(error.category(), ErrorCategory::Security);
    assert!(matches!(
        error,
        PlanError::UnsafeTemporaryArtifact { artifact_id: actual, .. } if actual == artifact_id
    ));
}
