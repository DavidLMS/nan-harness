use super::super::discovery::ProviderDiscovery;
use super::super::models::{
    DiagnosticLevel, ProviderReport, ProviderTextReport, coding_model_summaries,
};

pub(super) fn provider_json_report(discovery: ProviderDiscovery) -> ProviderReport {
    match discovery {
        ProviderDiscovery::NotConfigured => ProviderReport {
            level: DiagnosticLevel::Info,
            credential: "not-configured",
            api: "skipped",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::Invalid(code) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "invalid",
            api: "skipped",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some(code),
        },
        ProviderDiscovery::Models(models) => ProviderReport {
            level: DiagnosticLevel::Ok,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(models.len()),
            coding_models: coding_model_summaries(models),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::NoModels => ProviderReport {
            level: DiagnosticLevel::Warning,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(0),
            coding_models: Vec::new(),
            http_status: None,
            error_code: None,
        },
        ProviderDiscovery::Status(status) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: if matches!(status, 401 | 403) {
                "authentication-rejected"
            } else {
                "request-rejected"
            },
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: Some(status),
            error_code: Some("NH-PERSISTENCE-003"),
        },
        ProviderDiscovery::InvalidResponse => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "invalid-response",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-004"),
        },
        ProviderDiscovery::Unavailable(code) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "unavailable",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some(code),
        },
        ProviderDiscovery::Timeout => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "timeout",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-002"),
        },
    }
}

pub(super) fn provider_text_report(discovery: ProviderDiscovery) -> ProviderTextReport {
    match discovery {
        ProviderDiscovery::NotConfigured => ProviderTextReport::NotConfigured,
        ProviderDiscovery::Invalid(code) => ProviderTextReport::Invalid(code),
        ProviderDiscovery::Models(models) => ProviderTextReport::Models(models),
        ProviderDiscovery::NoModels => ProviderTextReport::NoModels,
        ProviderDiscovery::Status(status) => ProviderTextReport::Status(status),
        ProviderDiscovery::InvalidResponse => ProviderTextReport::InvalidResponse,
        ProviderDiscovery::Unavailable(code) => ProviderTextReport::Unavailable(code),
        ProviderDiscovery::Timeout => ProviderTextReport::Timeout,
    }
}
