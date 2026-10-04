//! Source-bound native Chat conversation input and assistant-only clipboard readback.
use super::{Gui, clipboard};
use crate::native::{CHAT_TURN_MAX_MILLIS, ChatActionPhase, failure_label};
use crate::native::{ChatTurnStage, GuardFailure};
use crate::provider::ProviderGate;
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
    row_shape: Option<crate::native::FailureRowShape>,
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
            row_shape: None,
            submitted_turns: 0,
            input_verified_turns: 0,
            copied_responses: 0,
            retry_attempted: false,
            clipboard_cleared: false,
        }
    }
}
pub(crate) struct ClaudeNativeChatSession<'a> {
    gui: &'a Gui,
    native_roots: &'a crate::probe::NativeRoots,
    destination: PathBuf,
    prompt: Zeroizing<String>,
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
            retry_ready: false,
            failure_details_attempted: false,
            facts: Facts::default(),
        })
    }
}
impl ClaudeNativeChatSession<'_> {
    fn observe_provider(&mut self, gate: &ProviderGate) {
        self.facts.provider_observation = Some(ProviderObservation {
            generation_observed: gate.generation_count() > 0,
            fixture_response_verified: gate.fixture_response_verified(),
            failure_observed: gate.failure_observed(),
        });
    }
    fn action(
        &mut self,
        mode: &str,
        marker: &str,
        deadline: Instant,
    ) -> Result<ChatTurnStage, Reason> {
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        self.facts.action_phase = Some(ChatActionPhase::BeforeGuard);
        self.facts.transport_failure = None;
        self.facts.guard_rejection = None;
        let sentinel = Zeroizing::new(nonce()?);
        let facts = &mut self.facts;
        let receipt = self.gui.visual.claude_chat_turn(
            mode,
            [&self.prompt, marker, &sentinel],
            deadline,
            |phase, failure, rejection| {
                facts.action_phase = Some(phase);
                facts.transport_failure = failure.map(failure_label);
                facts.guard_rejection = rejection.map(GuardRejection::from);
            },
        )?;
        let stage = receipt.stage;
        self.facts.stage = stage;
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
        if let Some(shape) = receipt.row_shape {
            self.facts.row_shape = Some(shape);
        }
        self.facts.action_phase = Some(ChatActionPhase::Completed);
        Ok(stage)
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
            "input-replace-owned",
            "",
            Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
        );
        match result {
            Ok(ChatTurnStage::Sent) => {
                self.facts.input_verified_turns += 1;
                self.facts.submitted_turns += 1;
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
                stage if stage.passive_pending() => {}
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
            match self.action(
                "retry-ready",
                "NAN_CHECK_EXPECTED_FAILURE",
                deadline
                    .min(Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS))),
            )? {
                ChatTurnStage::RetryReady => {
                    self.retry_ready = true;
                    return Ok(());
                }
                ChatTurnStage::ScopeAnchorAbsent
                    if gate.failure_observed() && !self.failure_details_attempted =>
                {
                    // Consume the disclosure before dispatch. An uncertain action
                    // cannot be replayed, and Retry still requires the raw marker.
                    self.failure_details_attempted = true;
                    if self.action(
                        "failure-details",
                        "NAN_CHECK_EXPECTED_FAILURE",
                        deadline.min(
                            Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
                        ),
                    )? != ChatTurnStage::FailureDetailsOpened
                    {
                        return Err(Reason::ActionUnsupported);
                    }
                }
                stage if stage.passive_pending() => {}
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
        if !std::mem::take(&mut self.retry_ready) || self.facts.retry_attempted {
            return Err(Reason::ActionUnsupported);
        }
        match self.action(
            "retry",
            "NAN_CHECK_EXPECTED_FAILURE",
            Instant::now() + Duration::from_millis(u64::from(CHAT_TURN_MAX_MILLIS)),
        ) {
            Ok(ChatTurnStage::Retried) => {
                self.facts.retry_attempted = true;
                Ok(())
            }
            Ok(ChatTurnStage::ActionUncertain) => {
                self.facts.retry_attempted = true;
                Err(Reason::ActionUnsupported)
            }
            Ok(_) => Err(Reason::ActionUnsupported),
            Err(reason) => {
                self.facts.retry_attempted = true;
                self.facts.stage = ChatTurnStage::ActionUncertain;
                Err(reason)
            }
        }
    }
    pub(crate) fn finish(
        mut self,
        gate: &ProviderGate,
        outcome: Result<(), Reason>,
    ) -> Result<(), Reason> {
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
