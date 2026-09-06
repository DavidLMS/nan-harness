use nan_harness_telemetry::event::{
    CompatibilityStatus, HarnessIdentity, HarnessKind, OperationContext, OperationKind, Transport,
};
use serde::Serialize;
use serde_json::{Value, json};

const HARNESS_KINDS: &[(HarnessKind, &str)] = &[
    (HarnessKind::ClaudeCode, "claude-code"),
    (HarnessKind::ChatGptDesktop, "chatgpt-desktop"),
    (HarnessKind::ClaudeDesktop, "claude-desktop"),
    (HarnessKind::Codex, "codex"),
    (HarnessKind::OpenCode, "opencode"),
    (HarnessKind::Hermes, "hermes"),
    (HarnessKind::HermesDesktop, "hermes-desktop"),
    (HarnessKind::PenDesktop, "pen-desktop"),
    (HarnessKind::ZedDesktop, "zed-desktop"),
    (HarnessKind::Pi, "pi"),
    (HarnessKind::Omp, "omp"),
    (HarnessKind::PrimeAgent, "prime-agent"),
    (HarnessKind::DeepSeekHarness, "deepseek-harness"),
    (HarnessKind::OpenClaw, "openclaw"),
    (HarnessKind::Cline, "cline"),
    (HarnessKind::QwenCode, "qwen-code"),
    (HarnessKind::KimiCode, "kimi-code"),
    (HarnessKind::Aider, "aider"),
    (HarnessKind::Goose, "goose"),
    (HarnessKind::Fx, "fx"),
];

const COMPATIBILITY_STATUSES: &[(CompatibilityStatus, &str)] = &[
    (CompatibilityStatus::Tested, "tested"),
    (CompatibilityStatus::Supported, "supported"),
    (CompatibilityStatus::NewerUntested, "newer-untested"),
    (CompatibilityStatus::OlderUnsupported, "older-unsupported"),
    (CompatibilityStatus::Unparseable, "unparseable"),
];

const TRANSPORTS: &[(Transport, &str)] = &[
    (Transport::DirectChat, "direct-chat"),
    (Transport::AnthropicBridge, "anthropic-bridge"),
    (Transport::ResponsesBridge, "responses-bridge"),
    (Transport::FxGatewayBridge, "fx-gateway-bridge"),
];

const OPERATION_KINDS: &[(OperationKind, &str)] = &[
    (OperationKind::HarnessRun, "harness-run"),
    (OperationKind::HarnessDryRun, "harness-dry-run"),
    (OperationKind::HarnessConfig, "harness-config"),
    (OperationKind::HarnessConfigRemove, "harness-config-remove"),
    (OperationKind::Doctor, "doctor"),
    (OperationKind::Update, "update"),
    (OperationKind::Uninstall, "uninstall"),
    (
        OperationKind::TelemetryConfiguration,
        "telemetry-configuration",
    ),
];

#[test]
fn harness_kind_as_str_names_are_stable() {
    for (kind, wire_name) in HARNESS_KINDS {
        assert_eq!(kind.as_str(), *wire_name);
    }
}

#[test]
fn harness_kind_json_names_are_stable() {
    for (kind, json_name) in HARNESS_KINDS {
        assert_eq!(serialized(*kind), Value::String((*json_name).to_owned()));

        let parsed: HarnessKind = serde_json::from_value(Value::String((*json_name).to_owned()))
            .expect("the JSON wire name should deserialize");
        assert_eq!(parsed, *kind);
    }

    assert!(
        serde_json::from_value::<HarnessKind>(Value::String("not-a-harness".to_owned())).is_err()
    );
}

#[test]
fn harness_identity_omits_unset_optional_fields() {
    let identity = HarnessIdentity::new(HarnessKind::Codex, None);
    let value = serde_json::to_value(&identity).expect("identity should serialize");
    let fields = value.as_object().expect("identity should be an object");

    assert_eq!(fields.get("kind"), Some(&Value::String("codex".to_owned())));
    assert!(!fields.contains_key("version"));
    assert!(!fields.contains_key("compatibility"));
    assert_eq!(fields.len(), 1);
}

#[test]
fn harness_identity_round_trips_optional_version_and_compatibility() {
    let identity = HarnessIdentity::new(HarnessKind::OpenCode, Some("2026.1".to_owned()))
        .with_compatibility(CompatibilityStatus::NewerUntested);
    let value = serde_json::to_value(&identity).expect("identity should serialize");

    assert_eq!(
        value,
        json!({
            "kind": "opencode",
            "version": "2026.1",
            "compatibility": "newer-untested",
        })
    );

    let parsed: HarnessIdentity =
        serde_json::from_value(value).expect("identity should deserialize");
    assert_eq!(parsed.kind(), HarnessKind::OpenCode);
    assert_eq!(parsed.version(), Some("2026.1"));
    assert_eq!(
        parsed.compatibility(),
        Some(CompatibilityStatus::NewerUntested)
    );
}

#[test]
fn harness_identity_rejects_unmodeled_context() {
    let value = json!({
        "kind": "codex",
        "model": "synthetic-model",
    });

    assert!(serde_json::from_value::<HarnessIdentity>(value).is_err());
}

#[test]
fn compatibility_status_wire_names_are_stable() {
    for (status, wire_name) in COMPATIBILITY_STATUSES {
        assert_eq!(status.as_str(), *wire_name);
        assert_eq!(serialized(*status), Value::String((*wire_name).to_owned()));

        let parsed: CompatibilityStatus =
            serde_json::from_value(Value::String((*wire_name).to_owned()))
                .expect("the documented wire name should deserialize");
        assert_eq!(parsed, *status);
    }
}

#[test]
fn transport_wire_names_are_stable() {
    for (transport, wire_name) in TRANSPORTS {
        assert_eq!(transport.as_str(), *wire_name);
        assert_eq!(
            serialized(*transport),
            Value::String((*wire_name).to_owned())
        );

        let parsed: Transport = serde_json::from_value(Value::String((*wire_name).to_owned()))
            .expect("the documented wire name should deserialize");
        assert_eq!(parsed, *transport);
    }
}

#[test]
fn operation_context_wire_names_are_stable() {
    for (operation, wire_name) in OPERATION_KINDS {
        assert_eq!(operation.as_str(), *wire_name);
        assert_eq!(
            serialized(*operation),
            Value::String((*wire_name).to_owned())
        );

        let parsed: OperationKind = serde_json::from_value(Value::String((*wire_name).to_owned()))
            .expect("the documented wire name should deserialize");
        assert_eq!(parsed, *operation);
    }

    let context = OperationContext::new(OperationKind::TelemetryConfiguration);
    assert_eq!(
        serde_json::to_value(&context).expect("operation context should serialize"),
        json!({"kind": "telemetry-configuration"}),
    );
    assert_eq!(context.kind(), OperationKind::TelemetryConfiguration);
}

fn serialized(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("schema value should serialize")
}

#[test]
fn legacy_chatgpt_spelling_is_accepted_but_serializes_canonically() {
    let kind: HarnessKind = serde_json::from_value(json!("chat-gpt-desktop"))
        .expect("previously emitted spelling remains readable");
    assert_eq!(kind, HarnessKind::ChatGptDesktop);
    assert_eq!(serialized(kind), json!("chatgpt-desktop"));
}

#[test]
fn every_harness_produces_a_report_matching_the_published_schema() {
    use nan_harness_telemetry::consent::{InstallationId, ReportConsent};
    use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
    use nan_harness_telemetry::event::{
        ErrorReport, ErrorReportContext, Failure, FailureCategory, FailureCause, FailureStage,
    };
    use nan_harness_telemetry::redaction::sanitize;

    let schema: Value = serde_json::from_str(include_str!(
        "../../../tests/telemetry/error-report.schema.json"
    ))
    .expect("published schema should parse");
    let validator = jsonschema::validator_for(&schema).expect("schema should compile");
    for (kind, wire_name) in HARNESS_KINDS {
        let context = ErrorReportContext::new(
            Failure::new(
                "NH-TEST-001",
                FailureCategory::Bridge,
                FailureStage::RequestTranslation,
                false,
            )
            .with_cause(FailureCause::InvalidResponse),
            false,
        )
        .with_harness(HarnessIdentity::new(*kind, None))
        .with_diagnostic(Diagnostic::general(DiagnosticReason::InvalidResponse));
        let report = ErrorReport::new(
            context,
            ReportConsent::one_time(),
            serde_json::from_value::<InstallationId>(json!(
                "installation_00000000000000000000000000000000"
            ))
            .expect("synthetic installation ID"),
        )
        .expect("report should build");
        let value =
            serialized(sanitize(report).expect("typed harness should satisfy privacy rules"));
        assert_eq!(value["harness"]["kind"], *wire_name);
        assert!(validator.is_valid(&value), "schema must accept {wire_name}");
    }
}
