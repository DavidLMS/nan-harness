//! Source-bound native Chat conversation input and assistant-only clipboard readback.
use super::{Gui, clipboard};
use crate::native::ChatTurnStage;
use crate::native::{ChatActionPhase, failure_label};
use crate::provider::ProviderGate;
use crate::report::Reason;
use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

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
    provider_observation: Option<ProviderObservation>,
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
            provider_observation: None,
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
        let sentinel = Zeroizing::new(nonce()?);
        let facts = &mut self.facts;
        let stage = self.gui.visual.claude_chat_turn(
            mode,
            [&self.prompt, marker, &sentinel],
            deadline,
            |phase, failure| {
                facts.action_phase = Some(phase);
                facts.transport_failure = failure.map(failure_label);
            },
        )?;
        self.facts.stage = stage;
        self.facts.action_phase = Some(ChatActionPhase::PostGuard);
        self.gui.visual.guard_observed(|category| {
            self.facts.transport_failure = Some(failure_label(category));
        })?;
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
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
        self.prompt = Zeroizing::new(prompt.to_owned());
        let result = self.action(
            "input-replace-owned",
            "",
            Instant::now() + Duration::from_secs(5),
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
            let until = deadline.min(Instant::now() + Duration::from_secs(5));
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
                deadline.min(Instant::now() + Duration::from_secs(5)),
            )? {
                ChatTurnStage::RetryReady => {
                    self.retry_ready = true;
                    return Ok(());
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
            Instant::now() + Duration::from_secs(5),
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
