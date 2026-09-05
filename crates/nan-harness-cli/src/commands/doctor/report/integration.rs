use super::super::discovery::IntegrationDiscovery;
use super::super::models::{
    ConfigurationTextReport, DiagnosticLevel, IntegrationReport, IntegrationSection,
};

pub(super) fn integration_json_report(discovery: IntegrationDiscovery) -> IntegrationSection {
    match discovery {
        IntegrationDiscovery::Failed { code, .. } => IntegrationSection {
            level: DiagnosticLevel::Error,
            integrations: Vec::new(),
            error_code: Some(code),
        },
        IntegrationDiscovery::Configured(integrations) => {
            let integrations = integrations
                .into_iter()
                .map(|integration| IntegrationReport {
                    id: integration.id,
                    active: integration.active,
                })
                .collect::<Vec<_>>();
            let level = if integrations.iter().all(|integration| integration.active) {
                DiagnosticLevel::Info
            } else {
                DiagnosticLevel::Warning
            };
            IntegrationSection {
                level,
                integrations,
                error_code: None,
            }
        }
    }
}

pub(super) fn configuration_text_report(
    discovery: IntegrationDiscovery,
) -> ConfigurationTextReport {
    match discovery {
        IntegrationDiscovery::Failed {
            subject,
            status,
            code,
        } => ConfigurationTextReport::Failed {
            subject,
            status,
            code,
        },
        IntegrationDiscovery::Configured(integrations) if integrations.is_empty() => {
            ConfigurationTextReport::NoneConfigured
        }
        IntegrationDiscovery::Configured(integrations) => ConfigurationTextReport::Configured(
            integrations
                .into_iter()
                .map(|integration| IntegrationReport {
                    id: integration.id,
                    active: integration.active,
                })
                .collect(),
        ),
    }
}
