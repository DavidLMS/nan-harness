//! Bounded, closed diagnostics emitted by the checker process.

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
    Process,
    NativeWindow,
    AccessibilityAttach,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuiAcquisitionDiagnostic {
    pub(crate) stage: GuiAcquisitionStage,
    pub(crate) error_category: ComposerErrorCategory,
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
    pub(crate) composer: Vec<ComposerFailure>,
    pub(crate) truncated: bool,
}

pub(crate) fn emit(mut event: DiagnosticEvent) {
    let mut bytes = serde_json::to_vec(&event).unwrap_or_default();
    if bytes.len() > MAX_LINE_BYTES {
        event.composer.clear();
        event.truncated = true;
        bytes = serde_json::to_vec(&event).unwrap_or_default();
    }
    if bytes.len() <= MAX_LINE_BYTES {
        eprintln!("DESKTOP_DIAGNOSTIC:{}", String::from_utf8_lossy(&bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Reason;

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
                stage: GuiAcquisitionStage::NativeWindow,
                error_category: ComposerErrorCategory::NativeHelperNonzeroExit,
            }),
            cleanup: None,
            composer: vec![],
            truncated: false,
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            serde_json::from_value::<DiagnosticEvent>(value).unwrap(),
            event
        );
        assert!(serde_json::from_value::<DiagnosticEvent>(serde_json::json!({"schemaVersion":1,"app":"chatgpt","mode":"live","launchStage":"started","composer":[],"truncated":false,"private":"x"})).is_err());
        let _ = Reason::NotRun;
    }
}
