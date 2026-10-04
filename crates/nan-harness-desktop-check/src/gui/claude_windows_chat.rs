//! Source-bound native Chat conversation input and assistant-only clipboard readback.
use super::{Gui, clipboard};
use crate::native::WINDOWS_CHAT_MAX_MILLIS;
use crate::native::{GuardFailure, WindowsChatStage};
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

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationTiming {
    budget_ms: u128,
    elapsed_ms: u128,
    transport_remaining_ms: Option<u128>,
    post_guard_remaining_ms: Option<u128>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    stage: WindowsChatStage,
    action_phase: Option<&'static str>,
    transport_failure: Option<&'static str>,
    operation_timing: OperationTiming,
    #[serde(skip_serializing_if = "Option::is_none")]
    guard_rejection: Option<GuardRejection>,
    provider_observation: Option<ProviderObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_scope_counts: Option<crate::native::FailureScopeCounts>,
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
            mechanism: "claude-windows-native-chat",
            diagnostics_only: true,
            stage: WindowsChatStage::Request,
            action_phase: None,
            transport_failure: None,
            operation_timing: OperationTiming::default(),
            guard_rejection: None,
            provider_observation: None,
            failure_scope_counts: None,
            submitted_turns: 0,
            input_verified_turns: 0,
            copied_responses: 0,
            retry_attempted: false,
            clipboard_cleared: false,
        }
    }
}
pub(crate) struct ClaudeWindowsChatSession<'a> {
    gui: &'a Gui,
    profile: &'a crate::probe::FreshClaudeWindowsProfile,
    workspace: &'a Path,
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
    pub(crate) fn claude_windows_chat_session<'a>(
        &'a self,
        directory: &Path,
        profile: &'a crate::probe::FreshClaudeWindowsProfile,
        workspace: &'a Path,
    ) -> Result<ClaudeWindowsChatSession<'a>, Reason> {
        if self.kind != nan_harness_core::DesktopHarnessKind::Claude
            || !super::claude_windows_ready::policy()
            || std::env::var("NANH_CLAUDE_WINDOWS_NATIVE_CHAT").as_deref() != Ok("1")
            || !profile.verifies_owned(workspace, Instant::now() + Duration::from_secs(1))
        {
            return Err(Reason::IsolationUnavailable);
        }
        let directory = super::qualification_directory::canonical_directory(directory)
            .ok_or(Reason::IsolationUnavailable)?;
        Ok(ClaudeWindowsChatSession {
            gui: self,
            profile,
            workspace,
            destination: directory.join(format!("claude-native-chat-{}.json", nonce()?)),
            prompt: Zeroizing::new(String::new()),
            retry_ready: false,
            failure_details_attempted: false,
            facts: Facts::default(),
        })
    }
}
impl ClaudeWindowsChatSession<'_> {
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
    ) -> Result<WindowsChatStage, Reason> {
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        let started = Instant::now();
        self.facts.operation_timing = OperationTiming {
            budget_ms: deadline.saturating_duration_since(started).as_millis(),
            ..OperationTiming::default()
        };
        self.facts.action_phase = Some("before-guard");
        self.facts.transport_failure = None;
        self.facts.guard_rejection = None;
        let sentinel = Zeroizing::new(nonce()?);
        if !self.profile.verifies_owned(self.workspace, deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        self.facts.action_phase = Some("transport");
        let facts = &mut self.facts;
        let result = self.gui.visual.claude_windows_chat_turn_until(
            mode,
            [&self.prompt, marker, &sentinel],
            deadline,
            |phase, failure, rejection| {
                facts.action_phase = Some(phase);
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis();
                match phase {
                    "transport" => {
                        facts
                            .operation_timing
                            .transport_remaining_ms
                            .get_or_insert(remaining);
                    }
                    "post-guard" => {
                        facts
                            .operation_timing
                            .post_guard_remaining_ms
                            .get_or_insert(remaining);
                    }
                    _ => {}
                }
                facts.transport_failure = failure.map(|value| match value {
                    crate::native::FailureCategory::InvalidInput => "invalid-input",
                    crate::native::FailureCategory::Spawn => "spawn",
                    crate::native::FailureCategory::Pipe => "pipe",
                    crate::native::FailureCategory::Output => "output",
                    crate::native::FailureCategory::Timeout => "timeout",
                    crate::native::FailureCategory::NonzeroExit => "nonzero-exit",
                    crate::native::FailureCategory::WindowChanged => "window-changed",
                    crate::native::FailureCategory::WindowQueryRejected => "window-query-rejected",
                    crate::native::FailureCategory::SessionUnavailable => "session-unavailable",
                });
                facts.guard_rejection = rejection.map(GuardRejection::from);
            },
        );
        self.facts.operation_timing.elapsed_ms = started.elapsed().as_millis();
        let stage = match result {
            Ok(receipt) => {
                if let Some(counts) = receipt.failure_scope {
                    self.facts.failure_scope_counts = Some(counts);
                }
                receipt.stage
            }
            Err(reason) => {
                self.facts.stage = WindowsChatStage::ActionUncertain;
                return Err(reason);
            }
        };
        self.facts.stage = stage;
        self.facts.action_phase = Some("post-guard");
        if !self.profile.verifies_owned(self.workspace, deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        self.facts.action_phase = Some("completed");
        Ok(stage)
    }
    pub(crate) fn new_turn(&mut self, prompt: &str) -> Result<(), Reason> {
        // Only the exclusive prelaunch private Windows root token can
        // authorize replacing a draft. Keep their original identity and privacy.
        if prompt.is_empty() || prompt.len() > 1024 || self.facts.submitted_turns >= 3 {
            return Err(Reason::InputMismatch);
        }
        self.retry_ready = false;
        self.prompt = Zeroizing::new(prompt.to_owned());
        let result = self.action(
            "input-replace-owned",
            "",
            Instant::now() + Duration::from_millis(u64::from(WINDOWS_CHAT_MAX_MILLIS)),
        );
        match result {
            Ok(WindowsChatStage::Sent) => {
                self.facts.input_verified_turns += 1;
                self.facts.submitted_turns += 1;
                Ok(())
            }
            Ok(WindowsChatStage::InputClipboardMismatch | WindowsChatStage::InputValueMismatch) => {
                Err(Reason::InputMismatch)
            }
            Ok(_) => Err(Reason::ActionUnsupported),
            Err(reason) => {
                self.facts.stage = WindowsChatStage::ActionUncertain;
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
                .min(Instant::now() + Duration::from_millis(u64::from(WINDOWS_CHAT_MAX_MILLIS)));
            match self.action("copy", marker, until)? {
                WindowsChatStage::Copied => {
                    // The helper already checked exact clipboard bytes; independently
                    // read them through the existing private clipboard transport.
                    if clipboard::read()?.as_str() != marker {
                        return Err(Reason::ResponseMismatch);
                    }
                    self.facts.copied_responses += 1;
                    return Ok(());
                }
                stage if stage.passive_pending() => {}
                WindowsChatStage::ResponseMismatch => return Err(Reason::ResponseMismatch),
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
                deadline.min(
                    Instant::now() + Duration::from_millis(u64::from(WINDOWS_CHAT_MAX_MILLIS)),
                ),
            )? {
                WindowsChatStage::RetryReady => {
                    self.retry_ready = true;
                    return Ok(());
                }
                WindowsChatStage::ScopeAnchorAbsent
                    if gate.failure_observed() && !self.failure_details_attempted =>
                {
                    // Consume the only disclosure before entering an uncertain Invoke.
                    self.failure_details_attempted = true;
                    if self.action(
                        "failure-details",
                        "NAN_CHECK_EXPECTED_FAILURE",
                        deadline.min(
                            Instant::now()
                                + Duration::from_millis(u64::from(WINDOWS_CHAT_MAX_MILLIS)),
                        ),
                    )? != WindowsChatStage::FailureDetailsOpened
                    {
                        return Err(Reason::ActionUnsupported);
                    }
                    // Disclosure never authorizes Retry; retain the raw marker proof.
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
            Instant::now() + Duration::from_millis(u64::from(WINDOWS_CHAT_MAX_MILLIS)),
        ) {
            Ok(WindowsChatStage::Retried) => {
                self.facts.retry_attempted = true;
                Ok(())
            }
            Ok(WindowsChatStage::ActionUncertain) => {
                self.facts.retry_attempted = true;
                Err(Reason::ActionUnsupported)
            }
            Ok(_) => Err(Reason::ActionUnsupported),
            Err(reason) => {
                self.facts.retry_attempted = true;
                self.facts.stage = WindowsChatStage::ActionUncertain;
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
            self.facts.stage = WindowsChatStage::ActionUncertain;
        }
        let outcome = outcome.and_then(|()| {
            if gate.fixture_response_verified() {
                Ok(())
            } else {
                Err(Reason::ProviderFailed)
            }
        });
        if outcome.is_ok() && cleanup.is_ok() {
            self.facts.stage = WindowsChatStage::Completed;
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
    fn closed_receipt_excludes_private_payloads() {
        let facts = Facts::default();
        let value = serde_json::to_value(facts).unwrap();
        assert_eq!(value["mechanism"], "claude-windows-native-chat");
        assert_eq!(value["submittedTurns"], 0);
        assert_eq!(value["copiedResponses"], 0);
        assert!(value.get("prompt").is_none());
        assert!(value.get("workspace").is_none());
        assert!(value.get("guardRejection").is_none());
    }

    #[test]
    fn clipboard_cleanup_requires_observed_empty_value() {
        assert_eq!(
            verified_clipboard_clear(|| Ok(()), || Ok(Zeroizing::new("PRIVATE".into()))),
            Err(Reason::CleanupFailed)
        );
        assert_eq!(
            verified_clipboard_clear(
                || Err(Reason::CleanupFailed),
                || panic!("read after failed clear")
            ),
            Err(Reason::CleanupFailed)
        );
        assert!(verified_clipboard_clear(|| Ok(()), || Ok(Zeroizing::new(String::new()))).is_ok());
    }
}
