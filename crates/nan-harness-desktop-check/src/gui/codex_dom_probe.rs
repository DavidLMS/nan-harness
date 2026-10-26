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

mod diagnostics;
use diagnostics::{RetryClickPhase, RetryClickRefusal, SpotlightObservation, ViewportReveal};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_click_phase: Option<RetryClickPhase>,
    #[serde(
        default,
        deserialize_with = "diagnostics::present",
        skip_serializing_if = "Option::is_none"
    )]
    retry_click_refusal: Option<RetryClickRefusal>,
    #[serde(
        default,
        deserialize_with = "diagnostics::present",
        skip_serializing_if = "Option::is_none"
    )]
    retry_guard_spotlight: Option<SpotlightObservation>,
    #[serde(
        default,
        deserialize_with = "diagnostics::present",
        skip_serializing_if = "Option::is_none"
    )]
    retry_viewport_reveal: Option<ViewportReveal>,
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
            && self.retry_click_refusal.is_none()
            && self.retry_guard_spotlight.is_none()
    }

    fn diagnostics_valid(&self) -> bool {
        (self.retry_viewport_reveal.is_none() || self.retry_control.confirmed())
            && self.retry_click_refusal.as_ref().map_or_else(
                || self.retry_guard_spotlight.is_none(),
                |refusal| {
                    matches!(self.error_category, Some(DriverError::ActionUncertain))
                        && self.retry_control.confirmed()
                        && !self.retry_completed.confirmed()
                        && refusal.valid(self.retry_click_phase)
                        && self
                            .retry_guard_spotlight
                            .as_ref()
                            .is_none_or(|spotlight| refusal.guard_rejected() && spotlight.valid())
                },
            )
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
    let recovery = matches!(turn.action, DomAction::Retry).then_some(provider);
    let outcome = run_driver(&driver, &request_path, &output_path, recovery, guard);
    std::fs::remove_file(request_path).map_err(|_| Reason::IsolationUnavailable)?;
    let facts = read_facts(&output_path).map_err(|reason| outcome.err().unwrap_or(reason))?;
    let provider_verified = provider.fixture_response_verified();
    let generation_count = provider.generation_count();
    let facts = close_facts(&output_path, facts, provider_verified, generation_count)?;
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
    let facts: Facts = serde_json::from_slice(&bytes).map_err(|_| Reason::IsolationUnavailable)?;
    if !facts.diagnostics_valid() {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(facts)
}

fn close_facts(
    path: &Path,
    mut facts: Facts,
    provider_verified: bool,
    generation_count: usize,
) -> Result<Facts, Reason> {
    facts.provider_response_verified = provider_verified.into();
    facts.provider_generation_count = Some(generation_count).filter(|n| *n <= 4096);
    let closed = path.with_extension("closed");
    open_private_new(&closed)
        .and_then(|file| serde_json::to_writer(file, &facts).map_err(std::io::Error::other))
        .and_then(|()| std::fs::rename(closed, path))
        .map_err(|_| Reason::IsolationUnavailable)?;
    Ok(facts)
}

fn run_driver(
    driver: &Path,
    request: &Path,
    output: &Path,
    recovery: Option<&ProviderGate>,
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
    let mut barrier = super::codex_recovery_barrier::RecoveryBarrier::new(request);
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
        if let Some(provider) = recovery
            && let Err(reason) = barrier.poll(|| provider.fail_recoverable_scenario(false))
        {
            break Err(reason);
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

    #[test]
    fn retry_diagnostics_survive_the_closed_provider_transport() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let raw = fixture["raw"].as_array().unwrap();
        let closed = fixture["closed"].as_array().unwrap();
        assert_eq!(raw.len(), 3);
        assert_eq!(raw.len(), closed.len());
        let directory = tempfile::tempdir().unwrap();
        for (index, (input, expected)) in raw.iter().zip(closed).enumerate() {
            let path = directory.path().join(format!("retry-{index}.json"));
            serde_json::to_writer(open_private_new(&path).unwrap(), input).unwrap();
            let facts = read_facts(&path).expect("actual Node Retry diagnostics must decode");
            assert!(!facts.owned());
            assert!(!facts.ui_verified(turn(DomAction::Retry, DomPurpose::Response)));
            let facts = close_facts(&path, facts, false, 4).unwrap();
            assert!(!facts.owned());
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                *expected
            );
            assert!(!read_facts(&path).unwrap().owned());
        }
    }

    #[test]
    fn every_produced_retry_refusal_keeps_the_closed_transport() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let ordinary = fixture["ordinary"].as_array().unwrap();
        assert_eq!(ordinary.len(), 45);
        let directory = tempfile::tempdir().unwrap();
        for (index, value) in ordinary.iter().enumerate() {
            let mut input = fixture["raw"][2].clone();
            let mut refusal = value.clone();
            input["retryClickPhase"] = refusal.as_object_mut().unwrap().remove("phase").unwrap();
            input["retryAttempted"] = (input["retryClickPhase"] == "post-guard").into();
            input["retryClickRefusal"] = refusal;
            let path = directory.path().join(format!("refusal-{index}.json"));
            serde_json::to_writer(open_private_new(&path).unwrap(), &input).unwrap();
            let facts = read_facts(&path).expect("actual ordinary Retry refusal must decode");
            let facts = close_facts(&path, facts, false, 4).unwrap();
            assert!(!facts.ui_verified(turn(DomAction::Retry, DomPurpose::Response)));
            input["providerResponseVerified"] = false.into();
            input["providerGenerationCount"] = 4.into();
            let output: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(output, input);
        }
    }

    #[test]
    fn retry_transport_rejects_private_fields_and_contradictory_diagnostics() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let base = &fixture["raw"][0];
        let directory = tempfile::tempdir().unwrap();
        let mutations = [
            ("/retryClickRefusal", serde_json::json!(null)),
            ("/retryGuardSpotlight", serde_json::json!(null)),
            (
                "/retryClickRefusal/reason",
                serde_json::json!("private-path"),
            ),
            ("/retryClickRefusal/remainingMs", serde_json::json!(600_001)),
            (
                "/retryClickRefusal/first/blocked",
                serde_json::json!("private-output"),
            ),
            (
                "/retryClickRefusal/first/ownedHitCount",
                serde_json::json!(10),
            ),
            (
                "/retryClickRefusal/first/testedViewportCount",
                serde_json::json!(null),
            ),
            ("/retryClickPhase", serde_json::json!("complete")),
            (
                "/retryGuardSpotlight/sourceVersion",
                serde_json::json!("private-value"),
            ),
            (
                "/retryGuardSpotlight/rendererSha256",
                serde_json::json!("private-value"),
            ),
            ("/retryGuardSpotlight/inputCount", serde_json::json!(4097)),
            (
                "/retryGuardSpotlight/toastAnnouncerCount",
                serde_json::json!(2),
            ),
            ("/retryGuardSpotlight/stableSamples", serde_json::json!(1)),
            ("/retryGuardSpotlight/failure", serde_json::json!("query")),
            ("/errorCategory", serde_json::json!("query-failed")),
            ("/retryControl", serde_json::json!(false)),
            ("/retryCompleted", serde_json::json!(true)),
        ];
        let mut malformed: Vec<_> = mutations
            .into_iter()
            .map(|(pointer, value)| {
                let mut changed = base.clone();
                *changed.pointer_mut(pointer).unwrap() = value;
                changed
            })
            .collect();
        for pointer in [
            "",
            "/retryClickRefusal",
            "/retryClickRefusal/first",
            "/retryGuardSpotlight",
        ] {
            let mut changed = base.clone();
            changed
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("rawPrivateOutput".to_owned(), "private".into());
            malformed.push(changed);
        }
        let mut missing = base.clone();
        missing["retryClickRefusal"]
            .as_object_mut()
            .unwrap()
            .remove("first");
        malformed.push(missing);
        let mut orphan = base.clone();
        orphan.as_object_mut().unwrap().remove("retryClickRefusal");
        malformed.push(orphan);
        for (index, value) in malformed.iter().enumerate() {
            let path = directory.path().join(format!("malformed-{index}.json"));
            serde_json::to_writer(open_private_new(&path).unwrap(), value).unwrap();
            assert!(read_facts(&path).is_err(), "malformed diagnostic {index}");
        }
    }

    #[test]
    fn viewport_reveal_is_a_preserved_witness_and_never_an_acceptance_oracle() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let raw = fixture["viewport"]["raw"].as_array().unwrap();
        let closed = fixture["viewport"]["closed"].as_array().unwrap();
        assert_eq!(raw.len(), 2);
        let directory = tempfile::tempdir().unwrap();
        for (index, (input, expected)) in raw.iter().zip(closed).enumerate() {
            let path = directory.path().join(format!("viewport-{index}.json"));
            serde_json::to_writer(open_private_new(&path).unwrap(), input).unwrap();
            let facts = read_facts(&path).unwrap();
            let retry = turn(DomAction::Retry, DomPurpose::Response);
            let mut without = input.clone();
            without
                .as_object_mut()
                .unwrap()
                .remove("retryViewportReveal");
            let legacy: Facts = serde_json::from_value(without).unwrap();
            assert_eq!(facts.ui_verified(retry), legacy.ui_verified(retry));
            assert_eq!(facts.ui_verified(retry), index == 0);
            let facts = close_facts(
                &path,
                facts,
                expected["providerResponseVerified"].as_bool().unwrap(),
                usize::try_from(expected["providerGenerationCount"].as_u64().unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(facts.ui_verified(retry), index == 0);
            let actual: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(actual, *expected);
        }
        for key in [
            "endpointOwned",
            "bindingVerified",
            "responseVerified",
            "retryAttempted",
            "retryCompleted",
            "userTurnObserved",
        ] {
            let mut missing = raw[0].clone();
            missing[key] = false.into();
            let facts: Facts = serde_json::from_value(missing).unwrap();
            assert!(!facts.ui_verified(turn(DomAction::Retry, DomPurpose::Response)));
        }
        let mut unowned = raw[0].clone();
        unowned["retryControl"] = false.into();
        let path = directory.path().join("unexpected-witness.json");
        serde_json::to_writer(open_private_new(&path).unwrap(), &unowned).unwrap();
        assert!(read_facts(&path).is_err());
    }

    #[test]
    fn retry_diagnostic_enums_require_strings_in_the_raw_transport() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let mut malformed = Vec::new();
        for (pointer, value) in [
            (
                "/retryClickPhase",
                serde_json::json!({"sample-second":null}),
            ),
            (
                "/retryClickRefusal/reason",
                serde_json::json!({"guard-rejected":null}),
            ),
            (
                "/retryGuardSpotlight/status",
                serde_json::json!({"verified":null}),
            ),
            (
                "/retryGuardSpotlight/sourceVersion",
                serde_json::json!({"26.1007.21434":null}),
            ),
            (
                "/retryGuardSpotlight/rendererSha256",
                serde_json::json!({"2b78662247051c8b0905e1afbe0839187b8267e9181e179bc422322e4010433b":null}),
            ),
        ] {
            let mut input = fixture["raw"][0].clone();
            *input.pointer_mut(pointer).unwrap() = value;
            malformed.push(input);
        }
        let mut blocked = fixture["raw"][2].clone();
        blocked["retryClickRefusal"]["reason"] = "sample-blocked".into();
        blocked["retryClickRefusal"]["second"] = serde_json::json!({
            "blocked":{"foreign-overlay":null},"ownedHitCount":null,"testedViewportCount":null
        });
        malformed.push(blocked);
        for (pointer, value) in [
            (
                "/retryGuardSpotlight/failure",
                serde_json::json!({"foreign-controls":null}),
            ),
            (
                "/retryGuardSpotlight/rejectionCheck",
                serde_json::json!({"toast-announcer":null}),
            ),
        ] {
            let mut input = fixture["raw"][1].clone();
            *input.pointer_mut(pointer).unwrap() = value;
            malformed.push(input);
        }
        let mut control = fixture["raw"][1].clone();
        control["retryGuardSpotlight"]["rejectionCheck"] = "control-location".into();
        control["retryGuardSpotlight"]["controlKind"] = serde_json::json!({"other":null});
        malformed.push(control);
        let mut reveal = fixture["viewport"]["raw"][0].clone();
        reveal["retryViewportReveal"] = serde_json::json!({"completed":null});
        malformed.push(reveal);
        let directory = tempfile::tempdir().unwrap();
        for (index, value) in malformed.iter().enumerate() {
            let path = directory.path().join(format!("object-enum-{index}.json"));
            serde_json::to_writer(open_private_new(&path).unwrap(), value).unwrap();
            assert!(read_facts(&path).is_err(), "object-form enum {index}");
        }
    }

    #[test]
    fn retry_diagnostics_never_authorize_a_forged_success_and_legacy_stays_valid() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/codex-retry-click.json"
        ))
        .unwrap();
        let mut value = fixture["raw"][0].clone();
        for key in [
            "endpointOwned",
            "targetVerified",
            "attached",
            "bindingVerified",
            "auxiliaryInert",
            "codingComposerReady",
            "uniqueComposer",
            "inputReadback",
            "inputSubmitted",
            "userTurnObserved",
            "responseVerified",
            "errorObserved",
            "retryControl",
            "retryAttempted",
            "retryCompleted",
        ] {
            value[key] = true.into();
        }
        value["assistantTurnCount"] = 1.into();
        value["errorCategory"] = serde_json::Value::Null;
        for removed in [None, Some("retryClickRefusal"), Some("retryGuardSpotlight")] {
            let mut forged = value.clone();
            if let Some(key) = removed {
                forged.as_object_mut().unwrap().remove(key);
            }
            let facts: Facts = serde_json::from_value(forged).unwrap();
            assert!(!facts.owned());
            for action in [DomAction::Ready, DomAction::Submit, DomAction::Retry] {
                assert!(!facts.ui_verified(turn(action, DomPurpose::Response)));
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.json");
        let mut legacy = fixture["raw"][0].clone();
        legacy.as_object_mut().unwrap().remove("retryClickRefusal");
        legacy
            .as_object_mut()
            .unwrap()
            .remove("retryGuardSpotlight");
        serde_json::to_writer(open_private_new(&path).unwrap(), &legacy).unwrap();
        let decoded = read_facts(&path).unwrap();
        assert!(decoded.retry_click_refusal.is_none());
        assert!(decoded.retry_guard_spotlight.is_none());
    }
}
