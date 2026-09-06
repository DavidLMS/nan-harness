use nan_harness_core::HarnessKind;
use nan_harness_core::model::{
    ModelQualification, QualificationMatrix, QualificationStatus, QualificationTransport,
};
use serde_json::Value;
use std::collections::BTreeSet;

const WIRE_FIELDS: [&str; 15] = [
    "claude-code",
    "codex",
    "opencode",
    "hermes",
    "pi",
    "omp",
    "prime-agent",
    "deepseek-harness",
    "openclaw",
    "cline",
    "qwen-code",
    "kimi-code",
    "aider",
    "goose",
    "fx",
];

type ExpectedQualification = (
    HarnessKind,
    QualificationStatus,
    QualificationTransport,
    &'static str,
);

const EXPECTED_QUALIFICATIONS: [ExpectedQualification; 15] = [
    (
        HarnessKind::ClaudeCode,
        QualificationStatus::Qualified,
        QualificationTransport::DirectChat,
        "2026-09-06/claude-code",
    ),
    (
        HarnessKind::Codex,
        QualificationStatus::Qualified,
        QualificationTransport::ResponsesBridge,
        "2026-09-06/codex",
    ),
    (
        HarnessKind::OpenCode,
        QualificationStatus::Unqualified,
        QualificationTransport::AnthropicBridge,
        "2026-09-06/opencode",
    ),
    (
        HarnessKind::Hermes,
        QualificationStatus::Unknown,
        QualificationTransport::FxGatewayBridge,
        "2026-09-06/hermes",
    ),
    (
        HarnessKind::Pi,
        QualificationStatus::Qualified,
        QualificationTransport::DirectChat,
        "2026-09-06/pi",
    ),
    (
        HarnessKind::Omp,
        QualificationStatus::Unqualified,
        QualificationTransport::ResponsesBridge,
        "2026-09-06/omp",
    ),
    (
        HarnessKind::PrimeAgent,
        QualificationStatus::Unknown,
        QualificationTransport::AnthropicBridge,
        "2026-09-06/prime-agent",
    ),
    (
        HarnessKind::DeepSeekHarness,
        QualificationStatus::Qualified,
        QualificationTransport::FxGatewayBridge,
        "2026-09-06/deepseek-harness",
    ),
    (
        HarnessKind::OpenClaw,
        QualificationStatus::Unqualified,
        QualificationTransport::DirectChat,
        "2026-09-06/openclaw",
    ),
    (
        HarnessKind::Cline,
        QualificationStatus::Unknown,
        QualificationTransport::ResponsesBridge,
        "2026-09-06/cline",
    ),
    (
        HarnessKind::QwenCode,
        QualificationStatus::Qualified,
        QualificationTransport::AnthropicBridge,
        "2026-09-06/qwen-code",
    ),
    (
        HarnessKind::KimiCode,
        QualificationStatus::Unqualified,
        QualificationTransport::FxGatewayBridge,
        "2026-09-06/kimi-code",
    ),
    (
        HarnessKind::Aider,
        QualificationStatus::Unknown,
        QualificationTransport::DirectChat,
        "2026-09-06/aider",
    ),
    (
        HarnessKind::Goose,
        QualificationStatus::Qualified,
        QualificationTransport::ResponsesBridge,
        "2026-09-06/goose",
    ),
    (
        HarnessKind::Fx,
        QualificationStatus::Unknown,
        QualificationTransport::AnthropicBridge,
        "2026-09-06/fx",
    ),
];

fn qualification_from_case(
    (_, status, transport, tested_at): ExpectedQualification,
) -> ModelQualification {
    ModelQualification {
        status,
        transport,
        tested_at: Some(tested_at.to_owned()),
    }
}

fn status_wire_name(status: QualificationStatus) -> &'static str {
    match status {
        QualificationStatus::Qualified => "qualified",
        QualificationStatus::Unqualified => "unqualified",
        QualificationStatus::Unknown => "unknown",
    }
}

fn transport_wire_name(transport: QualificationTransport) -> &'static str {
    match transport {
        QualificationTransport::DirectChat => "direct-chat",
        QualificationTransport::AnthropicBridge => "anthropic-bridge",
        QualificationTransport::ResponsesBridge => "responses-bridge",
        QualificationTransport::FxGatewayBridge => "fx-gateway-bridge",
    }
}

fn distinct_matrix() -> QualificationMatrix {
    let [
        claude_code,
        codex,
        opencode,
        hermes,
        pi,
        omp,
        prime_agent,
        deepseek_harness,
        openclaw,
        cline,
        qwen_code,
        kimi_code,
        aider,
        goose,
        fx,
    ] = EXPECTED_QUALIFICATIONS.map(qualification_from_case);

    QualificationMatrix {
        claude_code,
        codex,
        opencode,
        hermes,
        pi,
        omp,
        prime_agent,
        deepseek_harness,
        openclaw,
        cline,
        qwen_code,
        kimi_code,
        aider,
        goose,
        fx,
    }
}

#[test]
fn qualification_matrix_maps_every_stable_harness_to_its_profile() {
    let matrix = distinct_matrix();
    let expected_harnesses = EXPECTED_QUALIFICATIONS
        .iter()
        .map(|&(harness, _, _, _)| harness)
        .collect::<BTreeSet<_>>();
    assert_eq!(expected_harnesses, BTreeSet::from(HarnessKind::ALL));

    for &(harness, status, transport, tested_at) in &EXPECTED_QUALIFICATIONS {
        let qualification = matrix.for_harness(harness);
        assert_eq!(qualification.status, status, "wrong status for {harness:?}");
        assert_eq!(
            qualification.transport, transport,
            "wrong transport for {harness:?}"
        );
        assert_eq!(
            qualification.tested_at.as_deref(),
            Some(tested_at),
            "wrong tested_at mapping for {harness:?}"
        );
    }

    let tested_at_values = EXPECTED_QUALIFICATIONS
        .iter()
        .map(|&(_, _, _, tested_at)| tested_at)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tested_at_values.len(),
        EXPECTED_QUALIFICATIONS.len(),
        "mapping fixtures must be distinguishable"
    );
}

#[test]
fn qualification_matrix_round_trips_published_wire_fields_and_values() {
    let matrix = distinct_matrix();
    let wire = serde_json::to_value(&matrix).expect("serialize matrix");

    for (field, &(_, status, transport, tested_at)) in
        WIRE_FIELDS.iter().zip(EXPECTED_QUALIFICATIONS.iter())
    {
        let qualification = wire
            .get(field)
            .unwrap_or_else(|| panic!("missing wire field {field}"));
        assert_eq!(qualification["status"], status_wire_name(status));
        assert_eq!(qualification["transport"], transport_wire_name(transport));
        assert_eq!(
            qualification["testedAt"],
            Value::String(tested_at.to_owned())
        );
    }

    let decoded: QualificationMatrix =
        serde_json::from_value(wire).expect("decode published matrix");
    assert_eq!(decoded, matrix);
}
