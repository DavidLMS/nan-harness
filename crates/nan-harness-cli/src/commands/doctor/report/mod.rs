use super::discovery::SystemDiscovery;
use super::models::{PlatformReport, SystemDoctorReport, TextSystemReport};

mod experimental;
mod harness;
mod integration;
mod provider;
mod telemetry;

pub(crate) use experimental::{experimental_json_report, experimental_report};
pub(crate) use harness::{harness_details, harness_json_report};

pub(crate) fn system_json_report(discovery: SystemDiscovery) -> SystemDoctorReport {
    SystemDoctorReport {
        schema_version: super::models::DOCTOR_SCHEMA_VERSION,
        nan_harness_version: env!("CARGO_PKG_VERSION"),
        platform: PlatformReport {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        provider: provider::provider_json_report(discovery.provider),
        harnesses: harness::harness_json_reports(discovery.harnesses),
        experimental_harnesses: experimental::experimental_json_reports(
            discovery.experimental_harnesses,
        ),
        managed_configurations: integration::integration_json_report(
            discovery.managed_configurations,
        ),
        telemetry: telemetry::telemetry_json_report(discovery.telemetry),
        safe_to_share: true,
    }
}

pub(crate) fn system_text_report(discovery: SystemDiscovery) -> TextSystemReport {
    TextSystemReport {
        provider: provider::provider_text_report(discovery.provider),
        harnesses: harness::harness_text_reports(discovery.harnesses),
        experimental_harnesses: experimental::experimental_text_reports(
            discovery.experimental_harnesses,
        ),
        managed_configurations: integration::configuration_text_report(
            discovery.managed_configurations,
        ),
        telemetry: telemetry::telemetry_text_report(discovery.telemetry),
    }
}

#[cfg(test)]
mod tests {
    use super::super::discovery::{
        IntegrationDiscovery, ProviderDiscovery, SystemDiscovery, TelemetryDiscovery,
    };
    use super::super::models::{ConfigurationTextReport, ProviderTextReport, TelemetryTextReport};
    use super::*;

    #[test]
    fn system_text_report_preserves_section_delegation_boundaries() {
        let report = system_text_report(SystemDiscovery {
            provider: ProviderDiscovery::NotConfigured,
            harnesses: Vec::new(),
            experimental_harnesses: Vec::new(),
            managed_configurations: IntegrationDiscovery::Configured(Vec::new()),
            telemetry: TelemetryDiscovery::State(false),
        });
        assert!(matches!(report.provider, ProviderTextReport::NotConfigured));
        assert!(report.harnesses.is_empty());
        assert!(report.experimental_harnesses.is_empty());
        assert!(matches!(
            report.managed_configurations,
            ConfigurationTextReport::NoneConfigured
        ));
        assert!(matches!(
            report.telemetry,
            TelemetryTextReport::State(false)
        ));
    }
}
