use super::context::{telemetry_operation, telemetry_transport};
use super::identity::telemetry_harness;
use crate::app::{Cli, Command};
use crate::runner;
use nan_harness_telemetry::TelemetryReporter;
use nan_harness_telemetry::analytics::{DEFAULT_USAGE_EXPORT_TIMEOUT, UmamiExporter, UsageEvent};
use nan_harness_telemetry::event::{OperationKind, Transport as TelemetryTransport};
use nan_harness_telemetry::glitchtip::GlitchTipExporter;

pub(crate) fn start_usage_analytics(
    cli: &Cli,
    telemetry: Option<&TelemetryReporter<GlitchTipExporter>>,
) -> Option<tokio::task::JoinHandle<()>> {
    if matches!(cli.command, Command::Telemetry { .. }) {
        return None;
    }
    let installation_id = telemetry?
        .settings()
        .active_installation_id()
        .ok()
        .flatten()?;
    let base_url = configured_value(
        "NAN_HARNESS_UMAMI_URL",
        option_env!("NAN_HARNESS_UMAMI_URL"),
    )?;
    let website_id = configured_value(
        "NAN_HARNESS_UMAMI_WEBSITE_ID",
        option_env!("NAN_HARNESS_UMAMI_WEBSITE_ID"),
    )?;
    let exporter = UmamiExporter::new(&base_url, &website_id, DEFAULT_USAGE_EXPORT_TIMEOUT).ok()?;
    let operation = telemetry_operation(cli).kind();
    let transport = telemetry_transport(cli);
    let mut event = UsageEvent::new(telemetry_harness(cli), operation, transport);
    if operation == OperationKind::HarnessRun && transport == Some(TelemetryTransport::DirectChat) {
        event = event.with_chat_gateway(!runner::direct_chat_gateway_disabled(cli));
    }
    Some(tokio::spawn(async move {
        let _ = exporter.export(&installation_id, event).await;
    }))
}

fn configured_value(name: &str, embedded: Option<&str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => None,
        Ok(value) => Some(value),
        Err(_) => embedded
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}
