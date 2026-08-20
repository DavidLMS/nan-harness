#![forbid(unsafe_code)]

pub mod consent;
pub mod event;
pub mod glitchtip;
pub mod panic;
pub mod prompt;
pub mod redaction;

use consent::{ReportConsent, TelemetrySettingsStore};
use event::{ErrorReport, ErrorReportContext};
use glitchtip::ErrorReportExporter;
use panic::PendingReportStore;
use prompt::{PromptDecision, ask_to_send_error_report};
use std::io::{BufRead, Write};

pub const ERROR_REPORT_SENT_MESSAGE: &str = "Error report sent. Reference: ";
pub const ERROR_REPORT_QUEUED_MESSAGE: &str = "Error report queued for retry. Reference: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Sent,
    Declined,
    Deferred,
    Unavailable,
    Failed,
}

#[derive(Debug)]
pub struct TelemetryReporter<E> {
    settings: TelemetrySettingsStore,
    pending: PendingReportStore,
    exporter: Option<E>,
}

impl<E> TelemetryReporter<E>
where
    E: ErrorReportExporter,
{
    #[must_use]
    pub fn new(
        settings: TelemetrySettingsStore,
        pending: PendingReportStore,
        exporter: Option<E>,
    ) -> Self {
        Self {
            settings,
            pending,
            exporter,
        }
    }

    #[must_use]
    pub fn settings(&self) -> &TelemetrySettingsStore {
        &self.settings
    }

    #[must_use]
    pub fn pending(&self) -> &PendingReportStore {
        &self.pending
    }

    pub async fn report<R, W>(
        &self,
        context: ErrorReportContext,
        input: &mut R,
        output: &mut W,
    ) -> DeliveryOutcome
    where
        R: BufRead,
        W: Write,
    {
        let telemetry_enabled = self
            .settings
            .load()
            .is_ok_and(|settings| settings.enabled());
        if telemetry_enabled {
            let Some(report) = sanitized_report(context, ReportConsent::automatic()) else {
                return DeliveryOutcome::Failed;
            };
            let outcome = self.deliver(&report).await;
            write_delivery_status(&report, outcome, &self.pending, output);
            return outcome;
        }
        if !context.interactive() {
            return DeliveryOutcome::Deferred;
        }
        match ask_to_send_error_report(input, output) {
            Ok(PromptDecision::Send) => {
                let Some(report) = sanitized_report(context, ReportConsent::one_time()) else {
                    return DeliveryOutcome::Failed;
                };
                let outcome = self.deliver(&report).await;
                write_delivery_status(&report, outcome, &self.pending, output);
                outcome
            }
            Ok(PromptDecision::Decline) => DeliveryOutcome::Declined,
            Err(_) => DeliveryOutcome::Failed,
        }
    }

    pub async fn process_pending<R, W>(
        &self,
        interactive: bool,
        input: &mut R,
        output: &mut W,
    ) -> DeliveryOutcome
    where
        R: BufRead,
        W: Write,
    {
        let Some(report) = self.pending.load().ok().flatten() else {
            return DeliveryOutcome::Deferred;
        };
        let telemetry_enabled = self
            .settings
            .load()
            .is_ok_and(|settings| settings.enabled());
        if telemetry_enabled {
            let Some(report) = sanitize_existing(report, ReportConsent::automatic()) else {
                let _ = self.pending.delete();
                return DeliveryOutcome::Failed;
            };
            let outcome = self.deliver(&report).await;
            finalize_pending(&report, outcome, &self.pending, output);
            return outcome;
        }
        if !interactive {
            return DeliveryOutcome::Deferred;
        }
        match ask_to_send_error_report(input, output) {
            Ok(PromptDecision::Send) => {
                let Some(report) = sanitize_existing(report, ReportConsent::one_time()) else {
                    let _ = self.pending.delete();
                    return DeliveryOutcome::Failed;
                };
                let outcome = self.deliver(&report).await;
                finalize_pending(&report, outcome, &self.pending, output);
                outcome
            }
            Ok(PromptDecision::Decline) => {
                let _ = self.pending.delete();
                DeliveryOutcome::Declined
            }
            Err(_) => DeliveryOutcome::Failed,
        }
    }

    async fn deliver(&self, report: &redaction::SanitizedErrorReport) -> DeliveryOutcome {
        let Some(exporter) = &self.exporter else {
            return DeliveryOutcome::Unavailable;
        };
        match exporter.export(report).await {
            Ok(()) => DeliveryOutcome::Sent,
            Err(_) => DeliveryOutcome::Failed,
        }
    }
}

fn sanitized_report(
    context: ErrorReportContext,
    consent: ReportConsent,
) -> Option<redaction::SanitizedErrorReport> {
    ErrorReport::new(context, consent)
        .ok()
        .and_then(|report| redaction::sanitize(report).ok())
}

fn sanitize_existing(
    report: ErrorReport,
    consent: ReportConsent,
) -> Option<redaction::SanitizedErrorReport> {
    redaction::sanitize(report.with_consent(consent)).ok()
}

fn write_delivery_status<W: Write>(
    report: &redaction::SanitizedErrorReport,
    outcome: DeliveryOutcome,
    pending: &PendingReportStore,
    output: &mut W,
) {
    match outcome {
        DeliveryOutcome::Sent => {
            let _ = writeln!(
                output,
                "{ERROR_REPORT_SENT_MESSAGE}{}",
                report.as_report().report_id()
            );
        }
        DeliveryOutcome::Unavailable | DeliveryOutcome::Failed => {
            if pending.save(report).is_ok() {
                let _ = writeln!(
                    output,
                    "{ERROR_REPORT_QUEUED_MESSAGE}{}",
                    report.as_report().report_id()
                );
            }
        }
        DeliveryOutcome::Declined | DeliveryOutcome::Deferred => {}
    }
}

fn finalize_pending<W: Write>(
    report: &redaction::SanitizedErrorReport,
    outcome: DeliveryOutcome,
    pending: &PendingReportStore,
    output: &mut W,
) {
    if outcome == DeliveryOutcome::Sent {
        let _ = pending.delete();
        let _ = writeln!(
            output,
            "{ERROR_REPORT_SENT_MESSAGE}{}",
            report.as_report().report_id()
        );
    }
}
