use super::identity::telemetry_harness_identity;
use crate::app::{Cli, Command};
use nan_harness_telemetry::event::{
    ErrorReportContext, Failure, OperationContext, OperationKind, Transport as TelemetryTransport,
};

pub(crate) fn panic_telemetry_context(cli: &Cli, interactive: bool) -> ErrorReportContext {
    enrich_telemetry_context(
        ErrorReportContext::new(Failure::panic(), interactive),
        cli,
        HarnessIdentitySource::KindOnly,
    )
}

#[derive(Clone, Copy)]
pub(crate) enum HarnessIdentitySource<'a> {
    Detect,
    Known(&'a nan_harness_core::DetectedHarness),
    KindOnly,
}

pub(crate) fn enrich_telemetry_context(
    mut context: ErrorReportContext,
    cli: &Cli,
    harness_source: HarnessIdentitySource<'_>,
) -> ErrorReportContext {
    if let Some(harness) = telemetry_harness_identity(cli, harness_source) {
        context = context.with_harness(harness);
    }
    if let Some(transport) = telemetry_transport(cli) {
        context = context.with_transport(transport);
    }
    context.with_operation(telemetry_operation(cli))
}

pub(crate) fn is_harness_dry_run(cli: &Cli) -> bool {
    telemetry_operation(cli).kind() == OperationKind::HarnessDryRun
}

pub(super) fn telemetry_operation(cli: &Cli) -> OperationContext {
    match &cli.command {
        Command::ChatGptDesktop(arguments) => harness_operation(arguments.dry_run),
        Command::ClaudeDesktop(arguments) => harness_operation(arguments.dry_run),
        Command::HermesDesktop(arguments) => harness_operation(arguments.run.dry_run),
        Command::PenDesktop(arguments) => harness_operation(arguments.dry_run),
        Command::ZedDesktop(arguments) => harness_operation(arguments.dry_run),
        Command::Claude(arguments) | Command::Codex(arguments) | Command::Fx(arguments) => {
            harness_operation(arguments.run.dry_run)
        }
        Command::OpenCode(arguments)
        | Command::Hermes(arguments)
        | Command::Pi(arguments)
        | Command::Omp(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Qwen(arguments)
        | Command::Kimi(arguments)
        | Command::Aider(arguments)
        | Command::Goose(arguments) => harness_operation(arguments.run.dry_run),
        Command::Doctor(_) => OperationContext::new(OperationKind::Doctor),
        Command::Update | Command::Completions { .. } | Command::RecordInstallation(_) => {
            OperationContext::new(OperationKind::Update)
        }
        Command::Uninstall(_) => OperationContext::new(OperationKind::Uninstall),
        Command::Config(arguments) => {
            OperationContext::new(if arguments.remove || arguments.remove_all {
                OperationKind::HarnessConfigRemove
            } else {
                OperationKind::HarnessConfig
            })
        }
        Command::Auth { .. } | Command::Telemetry { .. } => {
            OperationContext::new(OperationKind::TelemetryConfiguration)
        }
    }
}

const fn harness_operation(dry_run: bool) -> OperationContext {
    OperationContext::new(if dry_run {
        OperationKind::HarnessDryRun
    } else {
        OperationKind::HarnessRun
    })
}

pub(super) const fn telemetry_transport(cli: &Cli) -> Option<TelemetryTransport> {
    match cli.command {
        Command::Claude(_) | Command::ClaudeDesktop(_) => Some(TelemetryTransport::AnthropicBridge),
        Command::Codex(_) | Command::ChatGptDesktop(_) => Some(TelemetryTransport::ResponsesBridge),
        Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::HermesDesktop(_)
        | Command::PenDesktop(_)
        | Command::ZedDesktop(_)
        | Command::Pi(_)
        | Command::Omp(_)
        | Command::Prime(_)
        | Command::DeepSeek(_)
        | Command::OpenClaw(_)
        | Command::Cline(_)
        | Command::Qwen(_)
        | Command::Kimi(_)
        | Command::Aider(_)
        | Command::Goose(_) => Some(TelemetryTransport::DirectChat),
        Command::Fx(_) => Some(TelemetryTransport::FxGatewayBridge),
        Command::Doctor(_)
        | Command::Config(_)
        | Command::Auth { .. }
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::Completions { .. }
        | Command::RecordInstallation(_) => None,
    }
}
