use nan_harness_core::launch_plan::LaunchId;
use nan_harness_core::{ErrorCategory, HarnessKind, LaunchPlan, PlanError, TransportKind};
use serde_json::{Value, json};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

#[test]
fn launch_id_accepts_the_documented_suffix_boundaries() {
    for suffix in ["a".repeat(12), "9".repeat(64)] {
        let value = format!("launch_{suffix}");
        let launch_id = LaunchId::new(value.clone()).expect("boundary ID should be valid");

        assert_eq!(launch_id.as_str(), value);
    }
}

#[test]
fn launch_id_rejects_invalid_lengths_prefix_and_characters() {
    for value in [
        format!("launch_{}", "a".repeat(11)),
        format!("launch_{}", "a".repeat(65)),
        format!("start_{}", "a".repeat(12)),
        format!("launch_{}", "A".repeat(12)),
        format!("launch_{}", "é".repeat(12)),
    ] {
        assert!(LaunchId::new(value).is_err());
    }
}

#[test]
fn launch_id_preserves_value_across_display_debug_and_json() {
    let launch_id = LaunchId::new("launch_a1b2c3d4e5f6").expect("ID should be valid");

    assert_eq!(launch_id.as_str(), "launch_a1b2c3d4e5f6");
    assert_eq!(launch_id.to_string(), "launch_a1b2c3d4e5f6");
    assert_eq!(
        format!("{launch_id:?}"),
        "LaunchId(\"launch_a1b2c3d4e5f6\")"
    );
    assert_eq!(
        serde_json::to_string(&launch_id).expect("ID should serialize"),
        "\"launch_a1b2c3d4e5f6\""
    );
    assert_eq!(
        serde_json::from_str::<LaunchId>("\"launch_a1b2c3d4e5f6\"")
            .expect("serialized ID should deserialize"),
        launch_id
    );
}

#[test]
fn launch_id_deserialization_rejects_invalid_values() {
    for json in ["\"launch_short\"", "\"launch_ABCDEFGHIJKL\"", "123", "null"] {
        assert!(serde_json::from_str::<LaunchId>(json).is_err());
    }
}

#[test]
fn launch_plan_defaults_optional_lists_and_ignores_unknown_top_level_fields() {
    let mut plan: Value = serde_json::from_str(DIRECT_PLAN).expect("fixture should be JSON");
    plan.as_object_mut()
        .expect("plan should be an object")
        .remove("configurationOverlays");
    plan.as_object_mut()
        .expect("plan should be an object")
        .remove("launchScopedFiles");
    plan["futureField"] = json!("ignored");

    let parsed: LaunchPlan = serde_json::from_value(plan).expect("compatible plan should parse");

    assert!(parsed.configuration_overlays.is_empty());
    assert!(parsed.launch_scoped_files.is_empty());
    assert!(
        !serde_json::to_value(parsed)
            .expect("plan should serialize")
            .as_object()
            .expect("serialized plan should be an object")
            .contains_key("futureField")
    );
}

#[test]
fn launch_plan_version_is_checked_by_typed_validation() {
    for schema_version in [1, 3] {
        let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid fixture");
        plan.schema_version = schema_version;

        assert!(matches!(
            nan_harness_core::LaunchPlanValidator::validate(&plan),
            Err(PlanError::InvalidField {
                field: "schemaVersion",
                message
            }) if message == "only schema version 2 is supported"
        ));
    }
}

#[test]
fn plan_error_codes_and_categories_are_stable() {
    let errors = [
        (
            PlanError::InvalidField {
                field: "schemaVersion",
                message: "bad".to_owned(),
            },
            "NH-PLAN-001",
            ErrorCategory::Contract,
        ),
        (
            PlanError::AdapterMismatch {
                adapter: HarnessKind::OpenCode,
                requested: HarnessKind::ClaudeCode,
            },
            "NH-PLAN-002",
            ErrorCategory::Contract,
        ),
        (
            PlanError::TransportMismatch {
                harness: HarnessKind::OpenCode,
                expected: TransportKind::DirectChat,
                actual: TransportKind::AnthropicBridge,
            },
            "NH-PLAN-003",
            ErrorCategory::Contract,
        ),
        (
            PlanError::MissingSecretReference {
                reference: "NAN_API_KEY".to_owned(),
            },
            "NH-PLAN-004",
            ErrorCategory::Security,
        ),
        (
            PlanError::ConflictingEnvironment {
                variable: "NAN_API_KEY".to_owned(),
            },
            "NH-PLAN-005",
            ErrorCategory::Security,
        ),
        (
            PlanError::UnsafeTemporaryArtifact {
                artifact_id: "config".to_owned(),
                reason: "unsafe".to_owned(),
            },
            "NH-PLAN-006",
            ErrorCategory::Security,
        ),
    ];

    for (error, code, category) in errors {
        assert_eq!(error.code(), code);
        assert_eq!(error.category(), category);
    }
}
