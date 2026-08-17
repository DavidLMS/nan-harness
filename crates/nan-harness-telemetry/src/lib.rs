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
            return self
                .deliver(ErrorReport::new(context, ReportConsent::automatic()))
                .await;
        }
        if !context.interactive() {
            return DeliveryOutcome::Deferred;
        }
        match ask_to_send_error_report(input, output) {
            Ok(PromptDecision::Send) => {
                self.deliver(ErrorReport::new(context, ReportConsent::one_time()))
                    .await
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
            let outcome = self
                .deliver(Ok(report.with_consent(ReportConsent::automatic())))
                .await;
            let _ = self.pending.delete();
            return outcome;
        }
        if !interactive {
            return DeliveryOutcome::Deferred;
        }
        match ask_to_send_error_report(input, output) {
            Ok(PromptDecision::Send) => {
                let outcome = self
                    .deliver(Ok(report.with_consent(ReportConsent::one_time())))
                    .await;
                let _ = self.pending.delete();
                outcome
            }
            Ok(PromptDecision::Decline) => {
                let _ = self.pending.delete();
                DeliveryOutcome::Declined
            }
            Err(_) => DeliveryOutcome::Failed,
        }
    }

    async fn deliver(&self, report: Result<ErrorReport, event::EventError>) -> DeliveryOutcome {
        let Ok(report) = report else {
            return DeliveryOutcome::Failed;
        };
        let Ok(report) = redaction::sanitize(report) else {
            return DeliveryOutcome::Failed;
        };
        let Some(exporter) = &self.exporter else {
            return DeliveryOutcome::Unavailable;
        };
        match exporter.export(&report).await {
            Ok(()) => DeliveryOutcome::Sent,
            Err(_) => DeliveryOutcome::Failed,
        }
    }
}
