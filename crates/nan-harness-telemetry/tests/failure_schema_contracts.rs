use nan_harness_telemetry::consent::{InstallationId, ReportConsent};
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
use nan_harness_telemetry::event::{
    ErrorReport, ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
};
use nan_harness_telemetry::redaction::sanitize;
use serde::Serialize;
use serde_json::{Value, json};

const FAILURE_CAUSES: &[(FailureCause, &str)] = &[
    (FailureCause::MissingExecutable, "missing-executable"),
    (FailureCause::NotFound, "not-found"),
    (FailureCause::UnsupportedVersion, "unsupported-version"),
    (FailureCause::MissingCredential, "missing-credential"),
    (FailureCause::InvalidConfiguration, "invalid-configuration"),
    (FailureCause::PermissionDenied, "permission-denied"),
    (FailureCause::Filesystem, "filesystem"),
    (FailureCause::Network, "network"),
    (FailureCause::Timeout, "timeout"),
    (FailureCause::HttpStatus, "http-status"),
    (FailureCause::InvalidResponse, "invalid-response"),
    (FailureCause::ProcessStart, "process-start"),
    (FailureCause::ProcessExit, "process-exit"),
    (FailureCause::Serialization, "serialization"),
    (FailureCause::InvalidData, "invalid-data"),
    (FailureCause::Internal, "internal"),
];

const FAILURE_CATEGORIES: &[(FailureCategory, &str)] = &[
    (FailureCategory::Configuration, "configuration"),
    (FailureCategory::Discovery, "discovery"),
    (FailureCategory::Planning, "planning"),
    (FailureCategory::Bridge, "bridge"),
    (FailureCategory::Provider, "provider"),
    (FailureCategory::Process, "process"),
    (FailureCategory::Tool, "tool"),
    (FailureCategory::Cleanup, "cleanup"),
    (FailureCategory::Internal, "internal"),
];

const FAILURE_STAGES: &[(FailureStage, &str)] = &[
    (FailureStage::Startup, "startup"),
    (FailureStage::CredentialResolution, "credential-resolution"),
    (FailureStage::ModelDiscovery, "model-discovery"),
    (FailureStage::HarnessDetection, "harness-detection"),
    (FailureStage::LaunchPlanning, "launch-planning"),
    (FailureStage::LaunchValidation, "launch-validation"),
    (FailureStage::BridgeStartup, "bridge-startup"),
    (FailureStage::RequestTranslation, "request-translation"),
    (FailureStage::HarnessExecution, "harness-execution"),
    (FailureStage::ToolExecution, "tool-execution"),
    (FailureStage::Shutdown, "shutdown"),
];

#[test]
fn failure_cause_as_str_names_are_canonical() {
    for (cause, wire_name) in FAILURE_CAUSES {
        assert_eq!(cause.as_str(), *wire_name);
    }
}

#[test]
fn failure_cause_json_names_round_trip() {
    for (cause, wire_name) in FAILURE_CAUSES {
        assert_eq!(serialized(*cause), Value::String((*wire_name).to_owned()));

        let parsed: FailureCause = serde_json::from_value(json!(wire_name))
            .expect("the canonical cause name should deserialize");
        assert_eq!(parsed, *cause);
    }
}

#[test]
fn failure_category_as_str_names_are_canonical() {
    for (category, wire_name) in FAILURE_CATEGORIES {
        assert_eq!(category.as_str(), *wire_name);
    }
}

#[test]
fn failure_category_json_names_round_trip() {
    for (category, wire_name) in FAILURE_CATEGORIES {
        assert_eq!(
            serialized(*category),
            Value::String((*wire_name).to_owned())
        );

        let parsed: FailureCategory = serde_json::from_value(json!(wire_name))
            .expect("the canonical category name should deserialize");
        assert_eq!(parsed, *category);
    }
}

#[test]
fn failure_stage_as_str_names_are_canonical() {
    for (stage, wire_name) in FAILURE_STAGES {
        assert_eq!(stage.as_str(), *wire_name);
    }
}

#[test]
fn failure_stage_json_names_round_trip() {
    for (stage, wire_name) in FAILURE_STAGES {
        assert_eq!(serialized(*stage), Value::String((*wire_name).to_owned()));

        let parsed: FailureStage = serde_json::from_value(json!(wire_name))
            .expect("the canonical stage name should deserialize");
        assert_eq!(parsed, *stage);
    }
}

#[test]
fn failure_preserves_explicit_retryability() {
    for retryable in [false, true] {
        let failure = Failure::new(
            "NH-TEST-001",
            FailureCategory::Provider,
            FailureStage::RequestTranslation,
            retryable,
        );

        assert_eq!(failure.code(), "NH-TEST-001");
        assert_eq!(failure.retryable(), retryable);
    }
}

#[test]
fn panic_failure_uses_the_internal_execution_contract() {
    let failure = Failure::panic();

    assert_eq!(failure.code(), "NH-INTERNAL-001");
    assert_eq!(failure.category(), FailureCategory::Internal);
    assert_eq!(failure.stage(), FailureStage::HarnessExecution);
    assert!(failure.is_panic());
    assert!(!failure.retryable());
    assert_eq!(failure.cause(), Some(FailureCause::Internal));
    assert_eq!(failure.http_status(), None);
    assert_eq!(
        serialized(failure),
        json!({
            "code": "NH-INTERNAL-001",
            "category": "internal",
            "stage": "harness-execution",
            "panic": true,
            "retryable": false,
            "cause": "internal",
        })
    );
}

#[test]
fn failure_omits_unset_optional_diagnostics() {
    let failure = Failure::new(
        "NH-TEST-001",
        FailureCategory::Configuration,
        FailureStage::Startup,
        false,
    );

    assert_eq!(failure.cause(), None);
    assert_eq!(failure.http_status(), None);
    assert_eq!(
        serialized(failure),
        json!({
            "code": "NH-TEST-001",
            "category": "configuration",
            "stage": "startup",
            "panic": false,
            "retryable": false,
        })
    );
}

#[test]
fn failure_round_trips_optional_diagnostics() {
    let failure = Failure::new(
        "NH-TEST-001",
        FailureCategory::Provider,
        FailureStage::RequestTranslation,
        true,
    )
    .with_cause(FailureCause::HttpStatus)
    .with_http_status(503);

    assert_eq!(failure.cause(), Some(FailureCause::HttpStatus));
    assert_eq!(failure.http_status(), Some(503));
    let value = serialized(&failure);
    assert_eq!(
        value,
        json!({
            "code": "NH-TEST-001",
            "category": "provider",
            "stage": "request-translation",
            "panic": false,
            "retryable": true,
            "cause": "http-status",
            "httpStatus": 503,
        })
    );
    assert_eq!(parsed_failure(&value), failure);
}

#[test]
fn every_failure_cause_has_a_safe_published_report_contract() {
    let validator = report_schema_validator();
    for (cause, wire_name) in FAILURE_CAUSES {
        let failure = schema_failure(*cause, FailureCategory::Provider);
        let value = schema_validated_report(&validator, failure, wire_name);

        assert_eq!(value["failure"]["cause"], *wire_name);
        assert_eq!(
            value["failure"].get("httpStatus").is_some(),
            *cause == FailureCause::HttpStatus
        );
        assert!(validator.is_valid(&value), "schema must accept {wire_name}");
    }
}

#[test]
fn every_failure_category_and_stage_has_a_safe_published_report_contract() {
    let validator = report_schema_validator();
    for category in FAILURE_CATEGORIES {
        let failure = Failure::new("NH-TEST-001", category.0, FailureStage::Startup, false)
            .with_cause(FailureCause::Network);
        let value = schema_validated_report(&validator, failure, category.1);
        assert_eq!(value["failure"]["category"], category.1);
    }

    for stage in FAILURE_STAGES {
        let failure = Failure::new("NH-TEST-001", FailureCategory::Provider, stage.0, false)
            .with_cause(FailureCause::Network);
        let value = schema_validated_report(&validator, failure, stage.1);
        assert_eq!(value["failure"]["stage"], stage.1);
    }
}

fn schema_failure(cause: FailureCause, category: FailureCategory) -> Failure {
    let failure = Failure::new(
        "NH-TEST-001",
        category,
        FailureStage::RequestTranslation,
        false,
    )
    .with_cause(cause);
    if cause == FailureCause::HttpStatus {
        failure.with_http_status(503)
    } else {
        failure
    }
}

fn safe_report(failure: Failure) -> ErrorReport {
    ErrorReport::new(
        ErrorReportContext::new(failure, false)
            .with_diagnostic(Diagnostic::general(DiagnosticReason::InvalidResponse)),
        ReportConsent::one_time(),
        serde_json::from_value::<InstallationId>(json!(
            "installation_00000000000000000000000000000000"
        ))
        .expect("synthetic installation ID should deserialize"),
    )
    .expect("synthetic report should build")
}

fn schema_validated_report(
    validator: &jsonschema::Validator,
    failure: Failure,
    label: &str,
) -> Value {
    let report = sanitize(safe_report(failure))
        .expect("synthetic failure report should satisfy the privacy allowlist");
    let value = serialized(&report);

    assert!(
        validator.is_valid(&value),
        "published schema must accept {label}"
    );
    value
}

fn report_schema_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../tests/telemetry/error-report.schema.json"
    ))
    .expect("published error-report schema should parse");
    jsonschema::validator_for(&schema).expect("error-report schema should compile")
}

fn parsed_failure(value: &Value) -> Failure {
    serde_json::from_value(value.clone()).expect("failure should deserialize")
}

fn serialized(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("failure schema value should serialize")
}
