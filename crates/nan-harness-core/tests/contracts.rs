use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CODEX_HOME_PLACEHOLDER, LaunchPlanValidator, LaunchScopedFile,
    PROVIDER_BASE_URL_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, TemporaryArtifactMode, Transport,
};
use nan_harness_core::{HarnessKind, LaunchPlan, ModelCatalog, ModelProfile, PlanError};
use serde_json::Value;
use std::collections::BTreeSet;
use std::str::FromStr;

const BRIDGE_PLAN: &str = include_str!("fixtures/launch-plan.bridge.json");
const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");
const MODEL_PROFILE: &str = include_str!("fixtures/model-profile.qwen3.6.json");

#[test]
fn launch_plan_examples_round_trip_without_schema_drift() {
    for source in [BRIDGE_PLAN, DIRECT_PLAN] {
        let expected: Value = serde_json::from_str(source).expect("fixture should be JSON");
        let plan: LaunchPlan = serde_json::from_str(source).expect("fixture should match Rust");
        let actual = serde_json::to_value(plan).expect("plan should serialize");

        assert_eq!(actual, expected);
    }
}

#[test]
fn model_profile_example_round_trips_without_schema_drift() {
    let expected: Value = serde_json::from_str(MODEL_PROFILE).expect("fixture should be JSON");
    let profile: ModelProfile =
        serde_json::from_str(MODEL_PROFILE).expect("fixture should match Rust");

    assert_eq!(
        serde_json::to_value(profile).expect("profile should serialize"),
        expected
    );
}

#[test]
fn accepted_examples_pass_semantic_validation() {
    for source in [BRIDGE_PLAN, DIRECT_PLAN] {
        let plan: LaunchPlan = serde_json::from_str(source).expect("fixture should match Rust");
        LaunchPlanValidator::validate(&plan).expect("fixture should be semantically valid");
    }
}

#[test]
fn validator_rejects_transport_environment_and_observability_violations() {
    let mut wrong_transport = direct_plan();
    wrong_transport.transport = bridge_plan().transport;
    assert!(matches!(
        LaunchPlanValidator::validate(&wrong_transport),
        Err(PlanError::TransportMismatch { .. })
    ));

    let mut conflicting_environment = direct_plan();
    conflicting_environment
        .environment
        .remove
        .insert("NAN_API_KEY".to_owned());
    assert!(matches!(
        LaunchPlanValidator::validate(&conflicting_environment),
        Err(PlanError::ConflictingEnvironment { .. })
    ));

    let mut payload_capture = direct_plan();
    payload_capture.observability.payload_capture = true;
    assert!(matches!(
        LaunchPlanValidator::validate(&payload_capture),
        Err(PlanError::InvalidField {
            field: "observability.payloadCapture",
            ..
        })
    ));
}

#[test]
fn model_resolution_keeps_explicit_unknown_models_usable() {
    let profile: ModelProfile = serde_json::from_str(MODEL_PROFILE).expect("valid profile");
    let catalog = ModelCatalog::new([profile]);
    let discovered = BTreeSet::from(["qwen3.6".to_owned(), "custom-model".to_owned()]);

    let known = catalog.resolve_explicit("qwen3.6", HarnessKind::ClaudeCode, &discovered);
    assert!(known.warnings.is_empty());

    let unknown = catalog.resolve_explicit("custom-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(unknown.resolved_id, "custom-model");
    assert_eq!(unknown.warnings.len(), 1);

    let undiscovered =
        catalog.resolve_explicit("private-model", HarnessKind::ClaudeCode, &discovered);
    assert_eq!(undiscovered.warnings.len(), 2);
}

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn bridge_plan() -> LaunchPlan {
    serde_json::from_str(BRIDGE_PLAN).expect("valid bridge plan fixture")
}

#[test]
fn direct_transport_fixture_stays_direct() {
    assert!(matches!(
        direct_plan().transport,
        Transport::DirectChat { .. }
    ));
}

#[test]
fn direct_transport_accepts_the_runtime_provider_url_placeholder() {
    let mut plan = direct_plan();
    let Transport::DirectChat { base_url, .. } = &mut plan.transport else {
        panic!("fixture should use direct chat");
    };
    PROVIDER_BASE_URL_PLACEHOLDER.clone_into(base_url);

    LaunchPlanValidator::validate(&plan).expect("runtime provider URL should be valid");
}

#[test]
fn validator_accepts_selected_model_placeholders_in_native_catalog_templates() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(format!(
        "{SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER} {SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER} {SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER} {SELECTED_MODEL_CAPABILITIES_PLACEHOLDER}"
    ));

    LaunchPlanValidator::validate(&plan)
        .expect("selected model metadata is a supported runtime template");
}

#[test]
fn validator_accepts_embedded_artifact_paths_and_rejects_unknown_references() {
    let mut plan = direct_plan();
    plan.process.arguments = vec![
        "--config".to_owned(),
        "catalog=\"{artifact:opencode-config}\"".to_owned(),
    ];
    LaunchPlanValidator::validate(&plan).expect("embedded artifact should be valid");

    plan.process.arguments[1] = "catalog=\"{artifact:missing-catalog}\"".to_owned();
    assert!(matches!(
        LaunchPlanValidator::validate(&plan),
        Err(PlanError::InvalidField {
            field: "process.arguments",
            ..
        })
    ));
}

#[test]
fn validator_requires_launch_scoped_files_to_use_an_owned_namespace() {
    let mut plan = direct_plan();
    plan.launch_scoped_files.push(LaunchScopedFile {
        id: "codex-profile".to_owned(),
        directory: CODEX_HOME_PLACEHOLDER.to_owned(),
        file_name: "nan-harness-launch_01contract.config.toml".to_owned(),
        ownership_prefix: "nan-harness-launch_".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: "model = \"qwen3.6\"\n".to_owned(),
        lifecycle: ArtifactLifecycle::Launch,
    });
    LaunchPlanValidator::validate(&plan).expect("owned launch-scoped file should be valid");

    plan.launch_scoped_files[0].file_name = "config.toml".to_owned();
    assert!(matches!(
        LaunchPlanValidator::validate(&plan),
        Err(PlanError::UnsafeTemporaryArtifact { .. })
    ));

    plan.launch_scoped_files[0].file_name = "codex-profile.config.toml".to_owned();
    plan.launch_scoped_files[0].ownership_prefix = "codex-".to_owned();
    assert!(matches!(
        LaunchPlanValidator::validate(&plan),
        Err(PlanError::UnsafeTemporaryArtifact { .. })
    ));
}

#[test]
fn extended_harness_names_have_stable_commands_and_aliases() {
    assert_eq!(HarnessKind::PrimeAgent.binary_name(), "prime-agent");
    assert_eq!(HarnessKind::DeepSeekHarness.binary_name(), "dsh");
    assert_eq!(HarnessKind::OpenClaw.binary_name(), "openclaw");
    assert_eq!(HarnessKind::Cline.binary_name(), "cline");
    assert_eq!(HarnessKind::QwenCode.binary_name(), "qwen");
    assert_eq!(HarnessKind::KimiCode.binary_name(), "kimi");
    assert_eq!(HarnessKind::Aider.binary_name(), "aider");
    assert_eq!(HarnessKind::Goose.binary_name(), "goose");
    assert_eq!(
        HarnessKind::from_str("prime").expect("prime alias should parse"),
        HarnessKind::PrimeAgent
    );
    assert_eq!(
        HarnessKind::from_str("dsh").expect("dsh alias should parse"),
        HarnessKind::DeepSeekHarness
    );
    assert_eq!(
        HarnessKind::from_str("deepseek").expect("deepseek command should parse"),
        HarnessKind::DeepSeekHarness
    );
    assert_eq!(
        HarnessKind::from_str("claw").expect("OpenClaw alias should parse"),
        HarnessKind::OpenClaw
    );
    assert_eq!(
        HarnessKind::from_str("qwen").expect("Qwen Code alias should parse"),
        HarnessKind::QwenCode
    );
    assert_eq!(
        HarnessKind::from_str("kimi").expect("Kimi Code alias should parse"),
        HarnessKind::KimiCode
    );
}
