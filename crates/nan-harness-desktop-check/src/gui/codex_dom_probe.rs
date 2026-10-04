//! Source-bound hosted Codex DOM controller; UI and provider oracles remain separate.

use super::{DomAction, DomPurpose, DomTurn};
use crate::process::ProbeProcess;
use crate::provider::ProviderGate;
use crate::report::Reason;
use nan_harness_private_fs::{open_private_new, open_private_read};
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    connection_path: std::path::PathBuf,
    main_binding_path: std::path::PathBuf,
    owner_pid: u32,
    prompt: &'a str,
    expected_marker: &'a str,
    timeout_ms: u32,
    action: DomAction,
    purpose: DomPurpose,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DriverError {
    OwnershipLost,
    ComposerUnavailable,
    StaleTurn,
    InputMismatch,
    ActionUncertain,
    RetryUnavailable,
    ResponseTimeout,
    QueryFailed,
    InvalidRequest,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PreAttachFailure {
    RequestJson,
    RequestPolicy,
    ConnectionRead,
    BindingRead,
    ConnectionSchema,
    BindingSchema,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ComposerAdmissionFailure {
    ScopeNotReady,
    NonuniqueEditor,
    MissingEditor,
    UnsupportedControl,
    DetachedOrInert,
    Disabled,
    Hidden,
    ForeignOverlay,
    PointerDisabled,
    AncestorLimit,
    HitUnavailable,
    SampleChanged,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComposerReadinessObservation {
    overflow: Evidence,
    home_composer_count: Option<u8>,
    pending_textarea_count: Option<u8>,
    prose_mirror_editable_count: Option<u8>,
    workspace_control_count: Option<u8>,
    editable_count: Option<u8>,
    codex_thread_count: Option<u8>,
    #[serde(rename = "classicChatGPTCount")]
    classic_chat_gpt_count: Option<u8>,
}

/// Closed evidence flags keep the wire format boolean without treating missing
/// evidence as a successful state.
#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(from = "bool", into = "bool")]
enum Evidence {
    #[default]
    Unconfirmed,
    Confirmed,
}

impl From<bool> for Evidence {
    fn from(value: bool) -> Self {
        if value {
            Self::Confirmed
        } else {
            Self::Unconfirmed
        }
    }
}

impl From<Evidence> for bool {
    fn from(value: Evidence) -> Self {
        matches!(value, Evidence::Confirmed)
    }
}

impl Evidence {
    fn confirmed(self) -> bool {
        bool::from(self)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Facts {
    schema_version: u8,
    mechanism: String,
    diagnostics_only: Evidence,
    endpoint_owned: Evidence,
    target_verified: Evidence,
    attached: Evidence,
    binding_verified: Evidence,
    auxiliary_inert: Evidence,
    coding_composer_ready: Evidence,
    unique_composer: Evidence,
    input_readback: Evidence,
    input_submitted: Evidence,
    user_turn_observed: Evidence,
    assistant_turn_count: usize,
    response_verified: Evidence,
    error_observed: Evidence,
    retry_control: Evidence,
    retry_attempted: Evidence,
    retry_completed: Evidence,
    error_category: Option<DriverError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pre_attach_failure: Option<PreAttachFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composer_admission_failure: Option<ComposerAdmissionFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composer_readiness_observation: Option<ComposerReadinessObservation>,
    #[serde(default)]
    provider_response_verified: Evidence,
    #[serde(default)]
    provider_generation_count: Option<usize>,
}

impl Facts {
    fn owned(&self) -> bool {
        self.schema_version == 1
            && self.mechanism == "codex-renderer-qualification"
            && self.diagnostics_only.confirmed()
            && self.endpoint_owned.confirmed()
            && self.target_verified.confirmed()
            && self.attached.confirmed()
            && self.binding_verified.confirmed()
            && self.auxiliary_inert.confirmed()
            && self.coding_composer_ready.confirmed()
            && self.assistant_turn_count <= 4096
            && self.error_category.is_none()
            && self.pre_attach_failure.is_none()
            && self.composer_admission_failure.is_none()
            && self.composer_readiness_observation.is_none()
    }

    fn ui_verified(&self, turn: DomTurn<'_>) -> bool {
        if !self.owned() {
            return false;
        }
        match turn.action {
            DomAction::Ready => {
                !self.input_submitted.confirmed() && !self.retry_attempted.confirmed()
            }
            DomAction::Submit => {
                self.unique_composer.confirmed()
                    && self.input_readback.confirmed()
                    && self.input_submitted.confirmed()
                    && self.user_turn_observed.confirmed()
                    && !self.retry_attempted.confirmed()
                    && match turn.purpose {
                        DomPurpose::Response => {
                            self.response_verified.confirmed() && self.assistant_turn_count > 0
                        }
                        DomPurpose::Failure => {
                            self.error_observed.confirmed() && self.retry_control.confirmed()
                        }
                    }
            }
            DomAction::Retry => {
                !self.input_submitted.confirmed()
                    && self.retry_control.confirmed()
                    && self.retry_attempted.confirmed()
                    && self.retry_completed.confirmed()
                    && self.user_turn_observed.confirmed()
                    && self.response_verified.confirmed()
                    && self.assistant_turn_count > 0
            }
        }
    }
    fn accepted(&self, turn: DomTurn<'_>, provider: &ProviderGate) -> bool {
        self.ui_verified(turn)
            && match (turn.action, turn.purpose) {
                (DomAction::Ready, _) => true,
                (_, DomPurpose::Response) => provider.fixture_response_verified(),
                (_, DomPurpose::Failure) => provider.failure_observed(),
            }
    }
}

pub(crate) struct CodexDomSession<'a> {
    process: &'a mut ProbeProcess,
    directory: &'a Path,
    owner: u32,
    reservation: TurnReservation,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    profile: Option<&'a crate::probe::FreshCodexProfile>,
    profile_deadline: Instant,
}

impl<'a> CodexDomSession<'a> {
    pub(crate) fn new(
        process: &'a mut ProbeProcess,
        directory: &'a Path,
        #[cfg(any(target_os = "linux", target_os = "macos", windows))] profile: Option<
            &'a crate::probe::FreshCodexProfile,
        >,
    ) -> Result<Self, Reason> {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("NANH_DESKTOP_RENDERER_APP").as_deref() != Ok("chatgpt-desktop")
            || !directory.is_absolute()
        {
            return Err(Reason::IsolationUnavailable);
        }
        let owner = process.id().ok_or(Reason::ApplicationExited)?;
        let mut session = Self {
            process,
            directory,
            owner,
            reservation: TurnReservation::default(),
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            profile,
            profile_deadline: Instant::now() + Duration::from_secs(50),
        };
        session.guard()?;
        Ok(session)
    }

    fn guard(&mut self) -> Result<(), Reason> {
        if Instant::now() >= self.profile_deadline {
            return Err(Reason::Timeout);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self
            .profile
            .is_some_and(|profile| !profile.verifies_owned(self.profile_deadline))
        {
            return Err(Reason::IsolationUnavailable);
        }
        if self.process.id() != Some(self.owner) {
            return Err(Reason::IsolationUnavailable);
        }
        if self
            .process
            .try_wait()
            .map_err(|_| Reason::ActionUnsupported)?
            .is_some()
        {
            return Err(Reason::ApplicationExited);
        }
        Ok(())
    }

    pub(crate) fn turn(
        &mut self,
        turn: DomTurn<'_>,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        self.profile_deadline = Instant::now() + Duration::from_secs(50);
        self.guard()?;
        self.reservation.reserve(turn)?;
        execute_turn(self.directory, self.owner, turn, provider, || self.guard())
    }
}

#[derive(Default)]
struct TurnReservation {
    failure_prompt: Option<String>,
    retry_started: bool,
    submitted_prompts: std::collections::BTreeSet<String>,
}

impl TurnReservation {
    fn reserve(&mut self, turn: DomTurn<'_>) -> Result<(), Reason> {
        if matches!(turn.action, DomAction::Submit)
            && (self.submitted_prompts.len() >= 3
                || !self.submitted_prompts.insert(turn.prompt.to_owned()))
        {
            return Err(Reason::IsolationUnavailable);
        }
        match (turn.action, turn.purpose) {
            (DomAction::Submit, DomPurpose::Failure) if self.failure_prompt.is_none() => {
                self.failure_prompt = Some(turn.prompt.to_owned());
                Ok(())
            }
            (DomAction::Retry, DomPurpose::Response)
                if !self.retry_started && self.failure_prompt.as_deref() == Some(turn.prompt) =>
            {
                // Reserve before the child starts; uncertainty never permits replay.
                self.retry_started = true;
                Ok(())
            }
            (DomAction::Ready | DomAction::Submit, DomPurpose::Response)
                if self.failure_prompt.is_none() =>
            {
                Ok(())
            }
            _ => Err(Reason::IsolationUnavailable),
        }
    }
}

fn execute_turn(
    directory: &Path,
    owner: u32,
    turn: DomTurn<'_>,
    provider: &ProviderGate,
    guard: impl FnMut() -> Result<(), Reason>,
) -> Result<(), Reason> {
    let driver = std::env::var_os("FEASIBILITY_CODEX_DOM_DRIVER")
        .map(std::path::PathBuf::from)
        .filter(|path| {
            path.is_absolute()
                && std::fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.file_type().is_file())
        })
        .ok_or(Reason::IsolationUnavailable)?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
    let stem = format!("codex-{}-{}", owner, u64::from_le_bytes(nonce));
    let request_path = directory.join(format!("{stem}.private"));
    let output_path = directory.join(format!("{stem}.json"));
    let request = Request {
        connection_path: directory.join(format!("connection-{owner}.json")),
        main_binding_path: directory.join(format!("main-binding-{owner}.private")),
        owner_pid: owner,
        prompt: turn.prompt,
        expected_marker: turn.marker,
        timeout_ms: 45_000,
        action: turn.action,
        purpose: turn.purpose,
    };
    open_private_new(&request_path)
        .and_then(|mut file| {
            let bytes = serde_json::to_vec(&request).map_err(std::io::Error::other)?;
            file.write_all(&bytes)
        })
        .map_err(|_| Reason::IsolationUnavailable)?;
    let outcome = run_driver(&driver, &request_path, &output_path, guard);
    std::fs::remove_file(request_path).map_err(|_| Reason::IsolationUnavailable)?;
    let mut facts = read_facts(&output_path).map_err(|reason| outcome.err().unwrap_or(reason))?;
    facts.provider_response_verified = provider.fixture_response_verified().into();
    facts.provider_generation_count = Some(provider.generation_count()).filter(|n| *n <= 4096);
    let closed = output_path.with_extension("closed");
    open_private_new(&closed)
        .and_then(|file| serde_json::to_writer(file, &facts).map_err(std::io::Error::other))
        .and_then(|()| std::fs::rename(closed, output_path))
        .map_err(|_| Reason::IsolationUnavailable)?;
    outcome?;
    if facts.accepted(turn, provider) {
        Ok(())
    } else {
        Err(Reason::ResponseMismatch)
    }
}

impl super::Gui {
    pub(super) fn probe_codex_dom(
        &self,
        directory: &Path,
        owner: u32,
        marker: &str,
        result: &mut crate::report::ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        let outcome = execute_turn(
            directory,
            owner,
            DomTurn {
                prompt: "Check this connection",
                marker,
                action: DomAction::Submit,
                purpose: DomPurpose::Response,
            },
            provider,
            || self.visual.guard(),
        );
        outcome?;
        result.steps.extend([
            crate::report::CheckStep::InputSubmitted,
            crate::report::CheckStep::ResponseVerified,
        ]);
        Ok(())
    }
}

fn read_facts(path: &Path) -> Result<Facts, Reason> {
    let mut bytes = Vec::new();
    open_private_read(path)
        .and_then(|(file, _)| file.take(16_385).read_to_end(&mut bytes))
        .map_err(|_| Reason::IsolationUnavailable)?;
    if bytes.len() > 16_384 {
        return Err(Reason::IsolationUnavailable);
    }
    serde_json::from_slice(&bytes).map_err(|_| Reason::IsolationUnavailable)
}

fn run_driver(
    driver: &Path,
    request: &Path,
    output: &Path,
    mut guard: impl FnMut() -> Result<(), Reason>,
) -> Result<(), Reason> {
    guard()?;
    let mut child = Command::new("node")
        .arg(driver)
        .arg("--qualify")
        .arg(request)
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::ActionUnsupported)?;
    let deadline = Instant::now() + Duration::from_secs(50);
    let result = loop {
        match child.try_wait() {
            Ok(Some(_)) => break guard(),
            Err(_) => break Err(Reason::ActionUnsupported),
            Ok(None) => {}
        }
        if let Err(reason) = guard() {
            break Err(reason);
        }
        if Instant::now() >= deadline {
            break Err(Reason::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if result.is_err() {
        let _ = child.kill();
        child.wait().map_err(|_| Reason::IsolationUnavailable)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(action: DomAction, purpose: DomPurpose) -> DomTurn<'static> {
        DomTurn {
            prompt: "Check the expected provider failure",
            marker: "synthetic",
            action,
            purpose,
        }
    }

    #[test]
    fn reservations_never_replay_uncertain_submission_or_retry_another_user_turn() {
        let mut state = TurnReservation::default();
        let failure = turn(DomAction::Submit, DomPurpose::Failure);
        assert_eq!(state.reserve(failure), Ok(()));
        assert_eq!(state.reserve(failure), Err(Reason::IsolationUnavailable));
        let retry = turn(DomAction::Retry, DomPurpose::Response);
        assert_eq!(
            state.reserve(DomTurn {
                prompt: "another user",
                ..retry
            }),
            Err(Reason::IsolationUnavailable)
        );
        assert_eq!(state.reserve(retry), Ok(()));
        assert_eq!(state.reserve(retry), Err(Reason::IsolationUnavailable));
        assert_eq!(
            TurnReservation::default().reserve(retry),
            Err(Reason::IsolationUnavailable)
        );
    }

    #[test]
    fn response_oracle_requires_real_owned_ui_turn_and_distinct_retry_action() {
        let value = serde_json::json!({
            "schemaVersion":1,"mechanism":"codex-renderer-qualification","diagnosticsOnly":true,
            "endpointOwned":true,"targetVerified":true,"attached":true,"bindingVerified":true,
            "auxiliaryInert":true,"codingComposerReady":true,"uniqueComposer":true,"inputReadback":true,
            "inputSubmitted":true,"userTurnObserved":true,"assistantTurnCount":1,"responseVerified":true,
            "errorObserved":false,"retryControl":false,"retryAttempted":false,"retryCompleted":false,
            "errorCategory":null
        });
        let mut facts: Facts = serde_json::from_value(value.clone()).unwrap();
        let response = turn(DomAction::Submit, DomPurpose::Response);
        assert!(facts.ui_verified(response));
        facts.input_readback = false.into();
        assert!(!facts.ui_verified(response));
        facts.input_readback = true.into();
        facts.assistant_turn_count = 0;
        assert!(!facts.ui_verified(response));
        facts.assistant_turn_count = 1;
        facts.input_submitted = false.into();
        facts.retry_control = true.into();
        facts.retry_attempted = true.into();
        let retry = turn(DomAction::Retry, DomPurpose::Response);
        assert!(!facts.ui_verified(retry));
        facts.retry_completed = true.into();
        assert!(facts.ui_verified(retry));
        facts.binding_verified = false.into();
        assert!(!facts.ui_verified(retry));
        for failure in [
            "request-json",
            "request-policy",
            "connection-read",
            "binding-read",
            "connection-schema",
            "binding-schema",
        ] {
            let mut receipt = value.clone();
            receipt["errorCategory"] = "invalid-request".into();
            receipt["attached"] = false.into();
            receipt["preAttachFailure"] = failure.into();
            let decoded: Facts = serde_json::from_value(receipt).unwrap();
            assert!(!decoded.ui_verified(response));
            assert_eq!(
                serde_json::to_value(decoded).unwrap()["preAttachFailure"],
                failure
            );
        }
        let mut receipt = value.clone();
        receipt["errorCategory"] = "composer-unavailable".into();
        receipt["composerAdmissionFailure"] = "scope-not-ready".into();
        receipt["composerReadinessObservation"] = serde_json::json!({
            "overflow":false,"homeComposerCount":0,"pendingTextareaCount":0,
            "proseMirrorEditableCount":0,"workspaceControlCount":0,"editableCount":1,
            "codexThreadCount":0,"classicChatGPTCount":1
        });
        let decoded: Facts = serde_json::from_value(receipt.clone()).unwrap();
        assert!(!decoded.ui_verified(response));
        let encoded = serde_json::to_value(decoded).unwrap();
        assert_eq!(
            encoded["composerReadinessObservation"],
            receipt["composerReadinessObservation"]
        );
        receipt["composerReadinessObservation"]["rawText"] = "private".into();
        assert!(serde_json::from_value::<Facts>(receipt).is_err());
        let mut unknown = value.clone();
        unknown["preAttachFailure"] = "private path".into();
        assert!(serde_json::from_value::<Facts>(unknown).is_err());
        let mut malformed = value;
        malformed["rawResponse"] = "private".into();
        assert!(serde_json::from_value::<Facts>(malformed).is_err());
    }
}
