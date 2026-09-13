//! Bounded, closed diagnostics emitted by the checker process.

use crate::report::Reason;
use crate::{
    gui::{ComposerErrorCategory, ComposerFailure},
    probe::{CleanupDiagnostic, LaunchExit},
};
use nan_harness_core::DesktopHarnessKind;
use serde::{Deserialize, Serialize};

const MAX_LINE_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProbeMode {
    Deterministic,
    Live,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchStage {
    NotStarted,
    Started,
    ExitedBeforeWindow,
    WindowUnavailable,
    WindowAcquired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GuiAcquisitionStage {
    ProcessLive,
    NativeHelper,
    WindowInventoryEmpty,
    WindowCandidates,
    WindowCandidatesEmpty,
    WindowCandidatesTooSmall,
    WindowOwnership,
    WindowStability,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuiAcquisitionDiagnostic {
    pub(crate) stage: GuiAcquisitionStage,
    pub(crate) error_category: ComposerErrorCategory,
    pub(crate) reason: Reason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WorkerResultFailure {
    Timeout,
    Wait,
    Cancelled,
    Missing,
    UnreadableOrOversized,
    Schema,
    ExitMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiagnosticEvent {
    pub(crate) schema_version: u8,
    pub(crate) app: DesktopHarnessKind,
    pub(crate) probe_index: Option<usize>,
    pub(crate) mode: ProbeMode,
    pub(crate) launch_stage: LaunchStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) launch_exit: Option<LaunchExit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gui_acquisition: Option<GuiAcquisitionDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cleanup: Option<CleanupDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result_reason: Option<Reason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) worker_result_failure: Option<WorkerResultFailure>,
    pub(crate) composer: Vec<ComposerFailure>,
    pub(crate) truncated: bool,
}

fn encode(mut event: DiagnosticEvent) -> Option<String> {
    let mut bytes = serde_json::to_vec(&event).ok()?;
    if bytes.len() > MAX_LINE_BYTES {
        event.composer.clear();
        event.truncated = true;
        bytes = serde_json::to_vec(&event).ok()?;
    }
    if bytes.len() <= MAX_LINE_BYTES {
        return String::from_utf8(bytes).ok();
    }
    None
}

pub(crate) fn emit(event: DiagnosticEvent) {
    if let Some(line) = encode(event) {
        eprintln!("DESKTOP_DIAGNOSTIC:{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Reason;

    #[test]
    fn native_events_match_the_shared_transport_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/desktop-diagnostics.json"
        ))
        .unwrap();
        for event in fixture["events"].as_array().unwrap() {
            if event["kind"] == "native" {
                let native: DiagnosticEvent =
                    serde_json::from_value(event["record"].clone()).unwrap();
                let emitted: serde_json::Value =
                    serde_json::from_str(&encode(native).unwrap()).unwrap();
                assert_eq!(emitted, event["record"]);
            }
        }
    }

    #[test]
    fn schema_is_closed_and_roundtrips() {
        let event = DiagnosticEvent {
            schema_version: 1,
            app: DesktopHarnessKind::ChatGpt,
            probe_index: Some(2),
            mode: ProbeMode::Deterministic,
            launch_stage: LaunchStage::ExitedBeforeWindow,
            launch_exit: Some(LaunchExit::Code(17)),
            gui_acquisition: Some(GuiAcquisitionDiagnostic {
                stage: GuiAcquisitionStage::NativeHelper,
                error_category: ComposerErrorCategory::NativeHelperNonzeroExit,
                reason: Reason::ActionUnsupported,
            }),
            cleanup: None,
            result_reason: None,
            worker_result_failure: None,
            composer: vec![],
            truncated: false,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            serde_json::from_value::<DiagnosticEvent>(value).unwrap(),
            event
        );
        assert!(serde_json::from_value::<DiagnosticEvent>(serde_json::json!({"schemaVersion":1,"app":"chatgpt","mode":"live","launchStage":"started","composer":[],"truncated":false,"private":"x"})).is_err());
    }

    #[test]
    fn oversized_composer_is_dropped_and_event_stays_bounded() {
        let event = DiagnosticEvent {
            schema_version: 1,
            app: DesktopHarnessKind::Pen,
            probe_index: Some(0),
            mode: ProbeMode::Deterministic,
            launch_stage: LaunchStage::WindowAcquired,
            launch_exit: None,
            gui_acquisition: None,
            cleanup: None,
            result_reason: Some(Reason::ResponseMismatch),
            worker_result_failure: None,
            composer: vec![
                ComposerFailure {
                    operation: crate::gui::ComposerOperation::TypeText,
                    error_category: ComposerErrorCategory::ActionUnsupported,
                };
                512
            ],
            truncated: false,
        };
        let bytes = serde_json::to_vec(&event).unwrap();
        assert!(bytes.len() > MAX_LINE_BYTES);
        let encoded = encode(event).unwrap();
        assert!(encoded.len() <= MAX_LINE_BYTES);
        let decoded: DiagnosticEvent = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.composer.is_empty());
        assert!(decoded.truncated);
        assert_eq!(decoded.result_reason, Some(Reason::ResponseMismatch));
    }
}
