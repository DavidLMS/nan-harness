use nan_harness_core::PlanError;
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    CLINE_MODEL_CATALOG_PLACEHOLDER, LaunchPlan, LaunchPlanValidator, NAN_SEARCH_BLOCK_BEGIN,
    NAN_SEARCH_BLOCK_END,
};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn invalid_field(error: &PlanError, field: &'static str, message: &str) {
    assert_eq!(
        error,
        &PlanError::InvalidField {
            field,
            message: message.to_owned(),
        }
    );
}

fn unsafe_artifact(error: &PlanError, artifact_id: &str, reason: &str) {
    assert_eq!(
        error,
        &PlanError::UnsafeTemporaryArtifact {
            artifact_id: artifact_id.to_owned(),
            reason: reason.to_owned(),
        }
    );
}

#[test]
fn artifact_references_accept_owned_ids_in_process_arguments_and_public_environment() {
    let mut plan = direct_plan();
    plan.process.arguments = vec!["--config={artifact:opencode-config}".to_owned()];
    plan.environment.public.insert(
        "OPENCODE_CONFIG_BACKUP".to_owned(),
        "{artifact:opencode-config}".to_owned(),
    );

    LaunchPlanValidator::validate(&plan).expect("owned artifact references should be valid");
}

#[test]
fn unknown_artifact_references_report_the_validated_field_and_reference() {
    let mut process_plan = direct_plan();
    process_plan.process.arguments = vec!["--config={artifact:missing-config}".to_owned()];
    let error = LaunchPlanValidator::validate(&process_plan).expect_err("unknown reference");
    invalid_field(
        &error,
        "process.arguments",
        "references unknown temporary artifact 'missing-config'",
    );

    let mut environment_plan = direct_plan();
    environment_plan.environment.public.insert(
        "OPENCODE_CONFIG_BACKUP".to_owned(),
        "{artifact:missing-config}".to_owned(),
    );
    let error = LaunchPlanValidator::validate(&environment_plan).expect_err("unknown reference");
    invalid_field(
        &error,
        "environment.public",
        "references unknown temporary artifact 'missing-config'",
    );
}

#[test]
fn malformed_artifact_references_report_the_original_field_and_value() {
    for value in [
        "{artifact:}",
        "{artifact:missing",
        "{artifact:missing{nested}}",
    ] {
        let mut plan = direct_plan();
        plan.process.arguments = vec![value.to_owned()];
        let error = LaunchPlanValidator::validate(&plan).expect_err("malformed reference");
        invalid_field(
            &error,
            "process.arguments",
            &format!("contains malformed artifact placeholder '{value}'"),
        );
    }
}

#[test]
fn omitted_runtime_feature_placeholders_are_allowed_in_each_template_resource() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(
        [
            AIDER_MODEL_METADATA_PLACEHOLDER,
            AIDER_MODEL_SETTINGS_PLACEHOLDER,
            CLINE_MODEL_CATALOG_PLACEHOLDER,
        ]
        .join(" "),
    );
    LaunchPlanValidator::validate(&plan).expect("supported feature placeholders should pass");

    let mut overlay_plan = direct_plan();
    overlay_plan.configuration_overlays.push(
        serde_json::from_value(serde_json::json!({
            "id": "feature-overlay",
            "pathHint": "feature-overlay",
            "sourcePath": "{runtime:user_home}",
            "files": [{
                "path": "config.toml",
                "mode": "0600",
                "contentTemplate": "{runtime:cline_model_catalog}",
                "policy": "replace"
            }],
            "lifecycle": "launch"
        }))
        .expect("valid overlay"),
    );
    LaunchPlanValidator::validate(&overlay_plan).expect("overlay feature placeholder should pass");

    let mut launch_file_plan = direct_plan();
    launch_file_plan.launch_scoped_files.push(
        serde_json::from_value(serde_json::json!({
            "id": "feature-launch-file",
            "directory": "{runtime:user_home}",
            "fileName": "nan-harness-feature-launch.toml",
            "ownershipPrefix": "nan-harness-",
            "mode": "0600",
            "contentTemplate": "{runtime:aider_model_settings}",
            "lifecycle": "launch"
        }))
        .expect("valid launch-scoped file"),
    );
    LaunchPlanValidator::validate(&launch_file_plan)
        .expect("launch-scoped feature placeholder should pass");
}

#[test]
fn search_blocks_allow_empty_and_multiple_ordered_blocks() {
    let mut plan = direct_plan();
    plan.temporary_artifacts[0].content_template = Some(format!(
        "before{NAN_SEARCH_BLOCK_BEGIN}{NAN_SEARCH_BLOCK_END}middle{NAN_SEARCH_BLOCK_BEGIN}second{NAN_SEARCH_BLOCK_END}after"
    ));

    LaunchPlanValidator::validate(&plan).expect("ordered search blocks should pass");
}

#[test]
fn search_block_order_count_and_nesting_errors_report_exact_artifact() {
    for template in [
        format!("{NAN_SEARCH_BLOCK_END}before"),
        format!("{NAN_SEARCH_BLOCK_BEGIN}missing-end"),
        format!("missing-begin{NAN_SEARCH_BLOCK_END}"),
        format!(
            "{NAN_SEARCH_BLOCK_BEGIN}one{NAN_SEARCH_BLOCK_BEGIN}nested{NAN_SEARCH_BLOCK_END}{NAN_SEARCH_BLOCK_END}"
        ),
        format!("{NAN_SEARCH_BLOCK_BEGIN}one{NAN_SEARCH_BLOCK_END}{NAN_SEARCH_BLOCK_END}"),
    ] {
        let mut plan = direct_plan();
        plan.temporary_artifacts[0].content_template = Some(template);
        let error = LaunchPlanValidator::validate(&plan).expect_err("malformed search blocks");
        unsafe_artifact(
            &error,
            "opencode-config",
            "contentTemplate contains malformed or nested NaN search blocks",
        );
    }
}

#[test]
fn malformed_runtime_and_secret_braces_report_exact_template_artifact() {
    for template in [
        "{runtime:missing",
        "{secret:missing",
        "{runtime:outer{secret:inner}}",
    ] {
        let mut plan = direct_plan();
        plan.temporary_artifacts[0].content_template = Some(template.to_owned());
        let error = LaunchPlanValidator::validate(&plan).expect_err("malformed placeholder");
        unsafe_artifact(
            &error,
            "opencode-config",
            "contentTemplate contains an unknown runtime or secret placeholder",
        );
    }
}
