//! Source-bound native Chat conversation input and assistant-only clipboard readback.
use super::{Gui, clipboard};
use crate::native::{CHAT_TURN_MAX_MILLIS, ChatActionPhase, failure_label};
use crate::native::{ChatTurnStage, GuardFailure};
use crate::provider::{ClaudeFailureEpoch, ProviderGate};
use crate::report::Reason;
use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum GuardRejection {
    IdentityMissing,
    BoundsChanged,
    ForegroundChanged,
    SameProcessWindow,
    OffDisplay,
    Occluded,
}
impl From<GuardFailure> for GuardRejection {
    fn from(failure: GuardFailure) -> Self {
        match failure {
            GuardFailure::IdentityMissing => Self::IdentityMissing,
            GuardFailure::BoundsChanged => Self::BoundsChanged,
            GuardFailure::ForegroundChanged => Self::ForegroundChanged,
            GuardFailure::SameProcessWindow => Self::SameProcessWindow,
            GuardFailure::OffDisplay => Self::OffDisplay,
            GuardFailure::Occluded => Self::Occluded,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderObservation {
    generation_observed: bool,
    fixture_response_verified: bool,
    failure_observed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    stage: ChatTurnStage,
    action_phase: Option<ChatActionPhase>,
    transport_failure: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guard_rejection: Option<GuardRejection>,
    provider_observation: Option<ProviderObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_authority: Option<crate::provider::FailureAuthorityObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    row_shape: Option<crate::native::FailureRowShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_shape: Option<crate::native::FailureScopeShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    composer_shape: Option<crate::native::ComposerShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_window_guard: Option<crate::native::WindowGuardObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_stage: Option<ChatTurnStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native_progress: Option<crate::native::MacChatProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport_started: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport_budget_ms: Option<u32>,
    submitted_turns: u8,
    input_verified_turns: u8,
    copied_responses: u8,
    retry_attempted: bool,
    clipboard_cleared: bool,
}
impl Default for Facts {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mechanism: "claude-native-chat",
            diagnostics_only: true,
            stage: ChatTurnStage::Request,
            action_phase: None,
            transport_failure: None,
            guard_rejection: None,
            provider_observation: None,
            failure_authority: None,
            row_shape: None,
            scope_shape: None,
            composer_shape: None,
            native_window_guard: None,
            native_stage: None,
            native_progress: None,
            transport_started: None,
            transport_budget_ms: None,
            submitted_turns: 0,
            input_verified_turns: 0,
            copied_responses: 0,
            retry_attempted: false,
            clipboard_cleared: false,
        }
    }
}
impl Facts {
    fn begin_action(&mut self, input: bool) {
        self.action_phase = Some(ChatActionPhase::BeforeGuard);
        self.transport_failure = None;
        self.guard_rejection = None;
        self.native_window_guard = None;
        self.native_stage = None;
        self.native_progress = None;
        self.transport_started = Some(false);
        self.transport_budget_ms = None;
        if input {
            self.composer_shape = None;
        }
    }
    fn observe_transport(&mut self, transport: crate::native::MacChatTransport) {
        self.native_progress = transport.progress;
        self.transport_started = Some(transport.started);
        self.transport_budget_ms = transport.budget_ms;
    }
}
struct SubmittedFailureTurn {
    epoch: ClaudeFailureEpoch,
}
pub(crate) struct ClaudeNativeChatSession<'a> {
    gui: &'a Gui,
    native_roots: &'a crate::probe::NativeRoots,
    destination: PathBuf,
    prompt: Zeroizing<String>,
    failure_epoch: Option<ClaudeFailureEpoch>,
    submitted_failure: Option<SubmittedFailureTurn>,
    retry_ready: bool,
    failure_details_attempted: bool,
    facts: Facts,
}
fn nonce() -> Result<String, Reason> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Reason::IsolationUnavailable)?;
    Ok(format!(
        "NAN_CHAT_CLIPBOARD_{:032x}",
        u128::from_le_bytes(bytes)
    ))
}
fn verified_clipboard_clear(
    clear: impl FnOnce() -> Result<(), Reason>,
    read: impl FnOnce() -> Result<Zeroizing<String>, Reason>,
) -> Result<(), Reason> {
    clear()?;
    if read()?.is_empty() {
        Ok(())
    } else {
        Err(Reason::CleanupFailed)
    }
}
impl Gui {
    pub(crate) fn claude_native_chat_session<'a>(
        &'a self,
        directory: &Path,
        native_roots: &'a crate::probe::NativeRoots,
    ) -> Result<ClaudeNativeChatSession<'a>, Reason> {
        if self.kind != nan_harness_core::DesktopHarnessKind::Claude
            || !crate::native::claude_focus_policy()
            || std::env::var("NANH_CLAUDE_MAC_NATIVE_CHAT").as_deref() != Ok("1")
            || directory.canonicalize().ok().as_deref() != Some(directory)
            || !native_roots.verifies_created_roots()
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(ClaudeNativeChatSession {
            gui: self,
            native_roots,
            destination: directory.join(format!("claude-native-chat-{}.json", nonce()?)),
            prompt: Zeroizing::new(String::new()),
            failure_epoch: None,
            submitted_failure: None,
            retry_ready: false,
            failure_details_attempted: false,
            facts: Facts::default(),
        })
    }
}
impl ClaudeNativeChatSession<'_> {
    fn observe_provider(&mut self, gate: &ProviderGate) {
        let epoch = self
            .submitted_failure
            .as_ref()
            .map(|turn| turn.epoch)
            .or(self.failure_epoch);
        self.facts.provider_observation = Some(ProviderObservation {
            generation_observed: gate.generation_count() > 0,
            fixture_response_verified: gate.fixture_response_verified(),
            failure_observed: epoch.is_some_and(|epoch| gate.claude_failure_turn_observed(epoch)),
        });
    }
    fn action(
        &mut self,
        mode: &str,
        marker: &str,
        deadline: Instant,
    ) -> Result<ChatTurnStage, Reason> {
        // Each invocation owns its diagnostics, including a pre-spawn refusal.
        self.facts.begin_action(matches!(
            mode,
            "input" | "input-replace-owned" | "input-failure-owned"
        ));
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        if !self.native_roots.verifies_created_roots() {
            return Err(Reason::IsolationUnavailable);
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        let sentinel = Zeroizing::new(nonce()?);
        let mut transport = crate::native::MacChatTransport::default();
        let facts = &mut self.facts;
        let result = self.gui.visual.claude_chat_turn(
            mode,
            [&self.prompt, marker, &sentinel],
            deadline,
            &mut transport,
            |phase, failure, rejection| {
                facts.action_phase = Some(phase);
                facts.transport_failure = failure.map(failure_label);
                facts.guard_rejection = rejection.map(GuardRejection::from);
            },
        );
        self.facts.observe_transport(transport);
        let receipt = result?;
        let stage = receipt.stage;
        self.facts.stage = stage;
        // Retain bounded native facts even if the independent post-guard fails.
        // These observations never turn an uncertain action into a success.
        self.facts.native_stage = Some(stage);
        if let Some(shape) = receipt.row_shape {
            self.facts.row_shape = Some(shape);
        }
        if let Some(shape) = receipt.scope_shape {
            self.facts.scope_shape = Some(shape);
        }
        if let Some(shape) = receipt.composer_shape {
            self.facts.composer_shape = Some(shape);
        }
        self.facts.native_window_guard = receipt.window_guard;
        self.facts.action_phase = Some(ChatActionPhase::PostGuard);
        self.gui
            .visual
            .claude_chat_guard_until(deadline, |failure, rejection| {
                self.facts.transport_failure = failure.map(failure_label);
                self.facts.guard_rejection = rejection.map(GuardRejection::from);
            })?;
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        self.facts.action_phase = Some(ChatActionPhase::Completed);
        Ok(stage)
    }
    pub(crate) fn prepare_turn(
        &mut self,
        prompt: &str,
        failure: bool,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        if self.facts.submitted_turns != self.facts.copied_responses
            || (failure && self.facts.submitted_turns != 2)
            || self.failure_epoch.is_some()
        {
            return Err(Reason::ActionUnsupported);
        }
        self.failure_epoch = gate.prepare_claude_turn(prompt, failure).map_err(|()| {
            if failure {
                self.facts.failure_authority = Some(gate.claude_failure_authority_rejection());
            }
            Reason::ProviderFailed
        })?;
        Ok(())
    }
    pub(crate) fn new_turn(&mut self, prompt: &str) -> Result<(), Reason> {
        // Only roots created after proving both native directories absent can
        // authorize replacing a draft. Keep their original identity and privacy.
        if !self.native_roots.verifies_created_roots() {
            return Err(Reason::IsolationUnavailable);
        }
        if prompt.is_empty() || prompt.len() > 1024 || self.facts.submitted_turns >= 3 {
            return Err(Reason::InputMismatch);
        }
        self.retry_ready = false;
        self.failure_details_attempted = false;
        self.prompt = Zeroizing::new(prompt.to_owned());
        let result = self.action(
            if self.failure_epoch.is_some() {
                "input-failure-owned"
            } else {
                "input-replace-owned"
            },
            "",
            Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
        );
        match result {
            Ok(ChatTurnStage::Sent) => {
                self.facts.input_verified_turns += 1;
                self.facts.submitted_turns += 1;
                self.submitted_failure = self
                    .failure_epoch
                    .take()
                    .map(|epoch| SubmittedFailureTurn { epoch });
                Ok(())
            }
            Ok(
                ChatTurnStage::InputMismatch
                | ChatTurnStage::InputInitialUnavailable
                | ChatTurnStage::InputInitialNonempty
                | ChatTurnStage::InputClipboardMismatch
                | ChatTurnStage::InputValueMismatch,
            ) => Err(Reason::InputMismatch),
            Ok(_) => Err(Reason::ActionUnsupported),
            Err(reason) => {
                self.facts.stage = ChatTurnStage::ActionUncertain;
                Err(reason)
            }
        }
    }
    pub(crate) fn wait_response(
        &mut self,
        marker: &str,
        timeout: Duration,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        let deadline = Instant::now() + timeout;
        loop {
            self.observe_provider(gate);
            let until = deadline
                .min(Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)));
            match self.action("copy", marker, until)? {
                ChatTurnStage::Copied => {
                    // The helper already checked exact clipboard bytes; independently
                    // read them through the existing private clipboard transport.
                    if clipboard::read()?.as_str() != marker {
                        return Err(Reason::ResponseMismatch);
                    }
                    self.facts.copied_responses += 1;
                    return Ok(());
                }
                stage if stage.pre_action_pending() => {}
                ChatTurnStage::ResponseMismatch => return Err(Reason::ResponseMismatch),
                _ => return Err(Reason::ActionUnsupported),
            }
            if Instant::now() >= deadline {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    pub(crate) fn wait_retry(
        &mut self,
        timeout: Duration,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        let deadline = Instant::now() + timeout;
        loop {
            self.observe_provider(gate);
            let authority = self
                .submitted_failure
                .as_ref()
                .is_some_and(|turn| gate.claude_failure_turn_observed(turn.epoch));
            if !authority {
                if Instant::now() >= deadline {
                    return Err(Reason::Timeout);
                }
                std::thread::sleep(
                    Duration::from_millis(100)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
                continue;
            }
            let observed = self.action(
                "retry-ready-temporal",
                "NAN_CHECK_EXPECTED_FAILURE",
                deadline
                    .min(Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS))),
            );
            self.observe_provider(gate);
            match observed? {
                ChatTurnStage::RetryReady => {
                    self.retry_ready = true;
                    return Ok(());
                }
                ChatTurnStage::ScopeAnchorAbsent
                    if gate.failure_observed() && !self.failure_details_attempted =>
                {
                    let ready = self.action(
                        "failure-details-ready-temporal",
                        "NAN_CHECK_EXPECTED_FAILURE",
                        deadline.min(
                            Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
                        ),
                    );
                    self.observe_provider(gate);
                    match ready?.disclosure_ready() {
                        Some(true) => {
                            // Consume only after passive proof. Dispatch uncertainty
                            // cannot be replayed; Retry still requires the raw marker.
                            self.failure_details_attempted = true;
                            let opened = self.action(
                                "failure-details-temporal",
                                "NAN_CHECK_EXPECTED_FAILURE",
                                deadline.min(
                                    Instant::now()
                                        + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
                                ),
                            );
                            self.observe_provider(gate);
                            if opened? != ChatTurnStage::FailureDetailsOpened {
                                return Err(Reason::ActionUnsupported);
                            }
                        }
                        Some(false) => {}
                        None => return Err(Reason::ActionUnsupported),
                    }
                }
                stage if stage.pre_action_pending() => {}
                _ => return Err(Reason::ActionUnsupported),
            }
            if Instant::now() >= deadline {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    pub(crate) fn retry_once(&mut self) -> Result<(), Reason> {
        if self.submitted_failure.is_none()
            || !std::mem::take(&mut self.retry_ready)
            || self.facts.retry_attempted
        {
            return Err(Reason::ActionUnsupported);
        }
        let deadline = Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS));
        loop {
            match self.action("retry-temporal", "NAN_CHECK_EXPECTED_FAILURE", deadline) {
                Ok(ChatTurnStage::Retried) => {
                    self.facts.retry_attempted = true;
                    return Ok(());
                }
                // The native helper has not pressed anything in this case.
                // Recheck the complete held scope within this one consumed intent.
                Ok(ChatTurnStage::Control) => {}
                Ok(ChatTurnStage::ActionUncertain) => {
                    self.facts.retry_attempted = true;
                    return Err(Reason::ActionUnsupported);
                }
                Ok(_) => return Err(Reason::ActionUnsupported),
                Err(reason) => {
                    self.facts.retry_attempted = true;
                    self.facts.stage = ChatTurnStage::ActionUncertain;
                    return Err(reason);
                }
            }
            if Instant::now() >= deadline {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    pub(crate) fn finish(
        mut self,
        gate: &ProviderGate,
        outcome: Result<(), Reason>,
    ) -> Result<(), Reason> {
        self.observe_provider(gate);
        if self.facts.submitted_turns == 3
            && self.facts.copied_responses == 2
            && !self.facts.retry_attempted
            && self
                .submitted_failure
                .as_ref()
                .is_some_and(|turn| !gate.claude_failure_turn_observed(turn.epoch))
        {
            self.facts.failure_authority = Some(gate.claude_failure_authority_rejection());
        }
        let cleanup = verified_clipboard_clear(|| clipboard::write(""), clipboard::read);
        self.facts.clipboard_cleared = cleanup.is_ok();
        if cleanup.is_err() {
            self.facts.stage = ChatTurnStage::ActionUncertain;
        }
        let outcome = outcome.and_then(|()| {
            if gate.fixture_response_verified() {
                Ok(())
            } else {
                Err(Reason::ProviderFailed)
            }
        });
        if outcome.is_ok() && cleanup.is_ok() {
            self.facts.stage = ChatTurnStage::Completed;
        }
        let bytes = serde_json::to_vec(&self.facts).map_err(|_| Reason::IsolationUnavailable)?;
        let recorded = open_private_new(&self.destination)
            .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()));
        cleanup.map_err(|_| Reason::CleanupFailed)?;
        recorded.map_err(|_| Reason::IsolationUnavailable)?;
        outcome
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_native_progress_survives_transport_timeout_in_closed_facts() {
        let (result, observed) = crate::native::mac_chat_progress_fixture();
        assert_eq!(result, Err(crate::native::FailureCategory::Timeout));
        let mut facts = Facts {
            submitted_turns: 2,
            input_verified_turns: 2,
            copied_responses: 2,
            ..Facts::default()
        };
        facts.begin_action(true);
        facts.observe_transport(observed);
        facts.stage = ChatTurnStage::ActionUncertain;
        facts.action_phase = Some(ChatActionPhase::Transport);
        facts.transport_failure = Some("timeout");
        let bytes = serde_json::to_vec(&facts).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["nativeProgress"], "tree");
        assert_eq!(value["transportStarted"], true);
        assert_eq!(value["transportBudgetMs"], 500);
        assert!(value.get("nativeStage").is_none());
        assert_eq!(value["submittedTurns"], 2);
        assert_eq!(value["retryAttempted"], false);
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../canary/tests/fixtures/claude-macos-transport-timeout.json"
        ))
        .unwrap();
        assert_eq!(value, expected);
        // Optional task-owned export lets the Python consumer validate the
        // actual C++ producer -> bounded transport -> production Facts chain.
        if let Some(path) = std::env::var_os("NANH_MAC_CHAT_DIAGNOSTIC_FIXTURE") {
            open_private_new(Path::new(&path))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
    #[test]
    fn transport_progress_is_closed_reset_per_attempt_and_never_counts_as_a_receipt() {
        let mut facts = Facts {
            native_stage: Some(ChatTurnStage::Sent),
            native_progress: Some(crate::native::MacChatProgress::Finish),
            transport_started: Some(true),
            transport_budget_ms: Some(1000),
            submitted_turns: 2,
            input_verified_turns: 2,
            copied_responses: 2,
            ..Facts::default()
        };
        facts.begin_action(true);
        let reset = serde_json::to_value(&facts).unwrap();
        assert_eq!(reset["actionPhase"], "before-guard");
        assert_eq!(reset["transportStarted"], false);
        for absent in [
            "nativeStage",
            "nativeProgress",
            "transportBudgetMs",
            "composerShape",
        ] {
            assert!(reset.get(absent).is_none());
        }
        facts.observe_transport(crate::native::MacChatTransport {
            progress: Some(crate::native::MacChatProgress::ReadbackCopy),
            started: true,
            budget_ms: Some(1234),
        });
        facts.stage = ChatTurnStage::ActionUncertain;
        facts.action_phase = Some(ChatActionPhase::Transport);
        facts.transport_failure = Some("timeout");
        let value = serde_json::to_value(&facts).unwrap();
        assert_eq!(value["nativeProgress"], "readback-copy");
        assert_eq!(value["transportStarted"], true);
        assert_eq!(value["transportBudgetMs"], 1234);
        assert!(value.get("nativeStage").is_none());
        assert!(value.get("composerShape").is_none());
        for count in ["submittedTurns", "inputVerifiedTurns", "copiedResponses"] {
            assert_eq!(value[count], 2);
        }
        assert_eq!(value["retryAttempted"], false);
        facts.begin_action(false);
        let next = serde_json::to_value(facts).unwrap();
        assert_eq!(next["transportStarted"], false);
        assert!(next.get("nativeProgress").is_none());
        assert!(next.get("transportBudgetMs").is_none());
    }
    #[test]
    fn guard_rejections_keep_transport_failure_distinct() {
        let mut facts = Facts::default();
        assert!(
            serde_json::to_value(&facts)
                .unwrap()
                .get("guardRejection")
                .is_none()
        );
        for (failure, label) in [
            (GuardFailure::IdentityMissing, "identity-missing"),
            (GuardFailure::BoundsChanged, "bounds-changed"),
            (GuardFailure::ForegroundChanged, "foreground-changed"),
            (GuardFailure::SameProcessWindow, "same-process-window"),
            (GuardFailure::OffDisplay, "off-display"),
            (GuardFailure::Occluded, "occluded"),
        ] {
            facts.action_phase = Some(ChatActionPhase::PostGuard);
            facts.guard_rejection = Some(failure.into());
            let value = serde_json::to_value(&facts).unwrap();
            assert_eq!(value["guardRejection"], label);
            assert!(value["transportFailure"].is_null());
        }
    }

    #[test]
    fn clipboard_cleanup_is_observed_even_on_driver_failure() {
        assert!(verified_clipboard_clear(|| Ok(()), || Ok(Zeroizing::new(String::new()))).is_ok());
        assert_eq!(
            verified_clipboard_clear(|| Ok(()), || Ok(Zeroizing::new("stale".into()))),
            Err(Reason::CleanupFailed)
        );
        assert_eq!(
            verified_clipboard_clear(
                || Err(Reason::ActionUnsupported),
                || panic!("failed clear must not certify cleanup")
            ),
            Err(Reason::ActionUnsupported)
        );
    }
    #[test]
    fn partial_receipts_never_imply_a_complete_conversation() {
        let value = serde_json::to_value(Facts::default()).unwrap();
        assert_eq!(value["stage"], "request");
        assert_eq!(value["submittedTurns"], 0);
        assert_eq!(value["clipboardCleared"], false);
        assert_eq!(value["diagnosticsOnly"], true);
    }
}
