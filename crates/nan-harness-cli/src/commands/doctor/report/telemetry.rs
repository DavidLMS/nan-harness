use super::super::discovery::TelemetryDiscovery;
use super::super::models::{DiagnosticLevel, TelemetryReport, TelemetryTextReport};

pub(super) fn telemetry_json_report(discovery: TelemetryDiscovery) -> TelemetryReport {
    match discovery {
        TelemetryDiscovery::State(enabled) => TelemetryReport {
            level: DiagnosticLevel::Info,
            enabled: Some(enabled),
            error_code: None,
        },
        TelemetryDiscovery::Failed => TelemetryReport {
            level: DiagnosticLevel::Error,
            enabled: None,
            error_code: Some("NH-TELEMETRY-001"),
        },
    }
}

pub(super) fn telemetry_text_report(discovery: TelemetryDiscovery) -> TelemetryTextReport {
    match discovery {
        TelemetryDiscovery::State(enabled) => TelemetryTextReport::State(enabled),
        TelemetryDiscovery::Failed => TelemetryTextReport::Failed,
    }
}
