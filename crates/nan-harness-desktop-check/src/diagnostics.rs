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

/// Closed launch failure sources. These identify the boundary that failed,
/// without copying command lines, paths, or child output into diagnostics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchFailure {
    #[serde(rename = "argument-validation-failed")]
    ArgumentValidation,
    #[serde(rename = "launch-setup-failed")]
    LaunchSetup,
    #[serde(rename = "provider-routing-failed")]
    ProviderRouting,
    #[serde(rename = "launcher-spawn-failed")]
    LauncherSpawn,
    #[serde(rename = "child-cli-failed")]
    ChildCli,
    NativeArgument,
    NativeCapabilityProbe,
    NativeCapabilityMissing,
    NativeCompatibility,
    NativeVersionProbe,
    NativeVersionUnparseable,
    NativeProcessInspection,
    NativeInstallation,
    NativeAlreadyRunning,
    NativeProfile,
    NativeModelCatalog,
    NativeBridgeHandshake,
    NativeAppExited,
    CredentialUnavailable,
    #[serde(rename = "native-app-spawn-failed")]
    NativeAppSpawn,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SetupCause {
    Discovery,
    Install,
    Configuration,
    Runtime,
    CurrentDirectory,
    CredentialInvariant,
    Preflight,
    InvalidPlan,
    SerializePlan,
    TelemetrySettings,
    Update,
    Persistence,
    Search,
    Uninstall,
    UsageEvidence,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DiscoveryCause {
    MissingExecutable,
    InvalidExecutable,
    InvalidManifest,
    MissingCompatibilityEntry,
    InvalidVersionCommand,
    VersionCommand,
    VersionCommandFailed,
    VersionProbeTimeout,
    VersionProbeOutputLimit,
    UnsupportedVersion,
    UnparseableVersion,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GuiAcquisitionStage {
    ProcessLive,
    NativeHelper,
    WindowInventoryEmpty,
    WindowOwnerNameMismatch,
    WindowCandidates,
    WindowCandidatesEmpty,
    WindowCandidatesTooSmall,
    WindowOwnership,
    WindowStability,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowInventoryObservation {
    Empty,
    Present,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AppNameObservation {
    Absent,
    Present,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GeometryObservation {
    EligibleAbsent,
    EligiblePresent,
    Mixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OwnershipObservation {
    Established,
    DifferentGroup,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CandidateFacts {
    pub(crate) inventory: WindowInventoryObservation,
    pub(crate) app_name: AppNameObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) geometry: Option<GeometryObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ownership: Option<OwnershipObservation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GuiAcquisitionDiagnostic {
    pub(crate) stage: GuiAcquisitionStage,
    pub(crate) error_category: ComposerErrorCategory,
    pub(crate) reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) foreground_relation: Option<crate::native::FitForegroundRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_facts: Option<CandidateFacts>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DisplayGeometryRelation {
    PartialMonitorOverlap,
    NoMonitorOverlap,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NativeProcessObservationState {
    MatchingProcessPresent,
    MatchingProcessAbsent,
    QueryFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeProcessObservation {
    pub(crate) state: NativeProcessObservationState,
    pub(crate) ever_observed_present: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ClaudeIdentityObservation {
    NoMatchingBundleProcess,
    MatchingProcessNoVisibleWindow,
    WindowNameMismatch,
    WindowNotEligible,
    WindowEligible,
    AmbiguousIdentity,
    QueryUnavailable,
    Overflow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ClaudeMatchedWindowInventory {
    Absent,
    PresentOffscreen,
    PresentOnscreen,
    QueryUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClaudeReadiness {
    pub(crate) finished_launching: Option<bool>,
    pub(crate) hidden: Option<bool>,
    pub(crate) active: Option<bool>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StartupHint {
    NoUsableSandbox,
    MissingSharedLibrary,
    DisplayUnavailable,
    Unknown,
    OutputUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartupDiagnostic {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit: Option<LaunchExit>,
    pub(crate) hint: StartupHint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sandbox: Option<SandboxFacts>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SandboxFacts {
    pub(crate) helper_presence: SandboxHelperPresence,
    pub(crate) helper_mode: SandboxHelperMode,
    pub(crate) helper_owner: SandboxHelperOwner,
    pub(crate) helper_location: SandboxHelperLocation,
    #[serde(rename = "apparmorUsernsRestriction")]
    pub(crate) apparmor_userns_restriction: NamespacePolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxHelperPresence {
    Present,
    Missing,
    Unreadable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxHelperMode {
    SetuidExecutable,
    ExecutableWithoutSetuid,
    NotExecutable,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxHelperOwner {
    Root,
    NonRoot,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxHelperLocation {
    SiblingPresentOrUnreadable,
    SiblingAbsent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NamespacePolicy {
    Restricted,
    Unrestricted,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiagnosticEvent {
    pub(crate) schema_version: u8,
    pub(crate) app: DesktopHarnessKind,
    pub(crate) probe_index: Option<usize>,
    pub(crate) mode: ProbeMode,
    pub(crate) launch_stage: LaunchStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_failure: Option<LaunchFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) setup_cause: Option<SetupCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovery_cause: Option<DiscoveryCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovery_exit: Option<LaunchExit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) launch_exit: Option<LaunchExit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) child_exit: Option<LaunchExit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) startup: Option<StartupDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gui_acquisition: Option<GuiAcquisitionDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) native_process_observation: Option<NativeProcessObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_identity_observation: Option<ClaudeIdentityObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matched_window_inventory: Option<ClaudeMatchedWindowInventory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claude_readiness: Option<ClaudeReadiness>,
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
    fn discovery_cause_fixture_is_typed_and_roundtrips() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/native-discovery-cause.json"
        ))
        .unwrap();
        let cause: DiscoveryCause =
            serde_json::from_value(value["discoveryCause"].clone()).unwrap();
        assert_eq!(
            serde_json::to_value(cause).unwrap(),
            value["discoveryCause"]
        );
    }

    #[test]
    fn schema_is_closed_and_roundtrips() {
        let event = DiagnosticEvent {
            schema_version: 1,
            app: DesktopHarnessKind::ChatGpt,
            probe_index: Some(2),
            mode: ProbeMode::Deterministic,
            launch_stage: LaunchStage::ExitedBeforeWindow,
            launch_failure: None,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            startup: None,
            launch_exit: Some(LaunchExit::Code(17)),
            child_exit: None,
            gui_acquisition: Some(GuiAcquisitionDiagnostic {
                stage: GuiAcquisitionStage::NativeHelper,
                error_category: ComposerErrorCategory::NativeHelperNonzeroExit,
                reason: Reason::ActionUnsupported,
                foreground_relation: None,
                candidate_facts: None,
            }),
            native_process_observation: None,
            claude_identity_observation: None,
            matched_window_inventory: None,
            claude_readiness: None,
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
    fn cli_sandbox_fixture_roundtrips_through_checker_schema() {
        let value: SandboxFacts = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/chatgpt-sandbox-diagnostic.json"
        ))
        .unwrap();
        let encoded = serde_json::to_string(&value).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/chatgpt-sandbox-diagnostic.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            expected
        );
    }

    #[test]
    fn launch_failure_sources_are_closed_and_safe() {
        let values = [
            (
                LaunchFailure::ArgumentValidation,
                "argument-validation-failed",
            ),
            (LaunchFailure::LaunchSetup, "launch-setup-failed"),
            (LaunchFailure::ProviderRouting, "provider-routing-failed"),
            (LaunchFailure::LauncherSpawn, "launcher-spawn-failed"),
            (LaunchFailure::ChildCli, "child-cli-failed"),
        ];
        for (source, expected) in values {
            assert_eq!(serde_json::to_value(source).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<LaunchFailure>(serde_json::json!(expected)).unwrap(),
                source
            );
        }
        assert!(
            serde_json::from_value::<LaunchFailure>(serde_json::json!("child-stderr")).is_err()
        );
    }

    #[test]
    fn oversized_composer_is_dropped_and_event_stays_bounded() {
        let event = DiagnosticEvent {
            schema_version: 1,
            app: DesktopHarnessKind::Pen,
            probe_index: Some(0),
            mode: ProbeMode::Deterministic,
            launch_stage: LaunchStage::WindowAcquired,
            launch_failure: None,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            startup: None,
            launch_exit: None,
            child_exit: None,
            gui_acquisition: None,
            native_process_observation: None,
            claude_identity_observation: None,
            matched_window_inventory: None,
            claude_readiness: None,
            cleanup: None,
            result_reason: Some(Reason::ResponseMismatch),
            worker_result_failure: None,
            composer: vec![
                ComposerFailure {
                    operation: crate::gui::ComposerOperation::TypeText,
                    error_category: ComposerErrorCategory::ActionUnsupported,
                    guard_context: None,
                    geometry_relation: None,
                    input_observation: None,
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
