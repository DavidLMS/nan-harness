//! Hosted-only native clipboard readback. No OCR or AX text reads.

use super::{Gui, WAIT, clipboard, map_error, primary_modifier};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, ProbeResult, Reason};
use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const RESPONSE_COPY: &str = "button[name=\"Copy This Agent Response\"], button[description=\"Copy This Agent Response\"], menu_item[name=\"Copy This Agent Response\"]";

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct InputFacts {
    entered: bool,
    clipboard_verified: bool,
    submitted: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseFacts {
    copy_action: bool,
    clipboard_verified: bool,
    provider_verified: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    navigation: &'static str,
    experiment_only: bool,
    ocr_used: bool,
    ax_text_used: bool,
    stage: &'static str,
    blocker: Option<Reason>,
    trust_control_count: Option<usize>,
    panel_control_count: Option<usize>,
    response_control_count: Option<usize>,
    clipboard_cleanup: &'static str,
    input: InputFacts,
    response: ResponseFacts,
}

fn nonce() -> Result<String, Reason> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| Reason::IsolationUnavailable)?;
    Ok(format!("{:016x}", u64::from_le_bytes(bytes)))
}

fn exact_readback(actual: &str, expected: &str, sentinel: &str) -> bool {
    actual == expected && actual != sentinel && !expected.is_empty()
}

fn control_count(control: &xa11y::Locator) -> Result<usize, Reason> {
    let count = control.count().map_err(map_error)?;
    if count > 4096 {
        return Err(Reason::ActionUnsupported);
    }
    Ok(count)
}

impl Gui {
    pub(crate) fn probe_native_copy(
        &self,
        directory: &Path,
        marker: &str,
        result: &mut ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        let mut facts = Facts {
            schema_version: 1,
            mechanism: "zed-native-copy",
            navigation: "private-keymap-new-thread",
            experiment_only: true,
            ocr_used: false,
            ax_text_used: false,
            stage: "trust",
            blocker: None,
            trust_control_count: None,
            panel_control_count: None,
            response_control_count: None,
            clipboard_cleanup: "not-run",
            input: InputFacts::default(),
            response: ResponseFacts::default(),
        };
        let outcome = self.run_native_copy(&mut facts, marker, result);
        facts.blocker = outcome.err();
        facts.response.provider_verified = provider.response_verified();
        let cleanup = clipboard::write("").and_then(|()| {
            if clipboard::read()?.is_empty() {
                Ok(())
            } else {
                Err(Reason::IsolationUnavailable)
            }
        });
        facts.clipboard_cleanup = if cleanup.is_ok() { "passed" } else { "failed" };
        let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
        let name = format!("{}-{}.json", std::process::id(), nonce()?);
        open_private_new(&directory.join(name))
            .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
            .map_err(|_| Reason::IsolationUnavailable)?;
        cleanup.and(outcome)
    }

    fn guarded_chord(&self, character: char, modifiers: &[xa11y::Key]) -> Result<(), Reason> {
        self.visual.guard()?;
        self.require_owned_foreground()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .keyboard()
            .chord(xa11y::Key::Char(character), modifiers)
            .map_err(map_error)?;
        self.visual.guard()
    }

    fn run_native_copy(
        &self,
        facts: &mut Facts,
        marker: &str,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let trust = app.locator("button[name=\"Trust and Continue\"]");
        let count = control_count(&trust)?;
        facts.trust_control_count = Some(count);
        match count {
            0 => {}
            1 => {
                self.visual.guard()?;
                trust.press().map_err(map_error)?;
                trust.wait_hidden(WAIT).map_err(map_error)?;
            }
            _ => return Err(Reason::SelectorNotMatched),
        }
        facts.stage = "panel";
        let panel = app.locator("*[name=\"Agent Panel\"]");
        let count = control_count(&panel)?;
        facts.panel_control_count = Some(count);
        if count > 1 {
            return Err(Reason::SelectorNotMatched);
        }
        // The private profile binds the global workspace NewThread handler,
        // which creates the draft and focuses its panel regardless of prior focus.
        self.guarded_chord('n', &[xa11y::Key::Ctrl, xa11y::Key::Alt])?;
        self.native_copy_input(facts)?;
        facts.stage = "submit";
        // Collapse selection before the one source-bound MessageEditor Chat action.
        self.visual.guard()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .keyboard()
            .press(xa11y::Key::ArrowRight)
            .map_err(map_error)?;
        self.require_owned_foreground()?;
        self.visual.guard()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .keyboard()
            .press(xa11y::Key::Enter)
            .map_err(map_error)?;
        facts.input.submitted = true;
        result.steps.push(CheckStep::InputSubmitted);
        self.native_copy_response(facts, marker)
    }

    fn native_copy_input(&self, facts: &mut Facts) -> Result<(), Reason> {
        facts.stage = "input";
        let prompt = Zeroizing::new(format!(
            "Check this connection. Read read-target.txt. Input nonce {}.",
            nonce()?
        ));
        self.guarded_chord('a', &[primary_modifier()])?;
        self.visual.guard()?;
        self.require_owned_foreground()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .keyboard()
            .type_text(&prompt)
            .map_err(map_error)?;
        facts.input.entered = true;
        let sentinel = Zeroizing::new(format!("clipboard-sentinel-{}", nonce()?));
        clipboard::write(&sentinel)?;
        self.guarded_chord('a', &[primary_modifier()])?;
        self.guarded_chord('c', &[primary_modifier()])?;
        facts.input.clipboard_verified = self.wait_native_copy(&prompt, &sentinel)?;
        if facts.input.clipboard_verified {
            Ok(())
        } else {
            Err(Reason::InputMismatch)
        }
    }

    fn native_copy_response(&self, facts: &mut Facts, marker: &str) -> Result<(), Reason> {
        facts.stage = "response-control";
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let control = app.locator(RESPONSE_COPY);
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            self.visual.guard()?;
            let count = control_count(&control)?;
            facts.response_control_count = Some(count);
            if count == 1 {
                break;
            }
            if count > 1 || Instant::now() >= deadline {
                return Err(Reason::SelectorNotMatched);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        control.wait_visible(WAIT).map_err(map_error)?;
        let sentinel = Zeroizing::new(format!("response-sentinel-{}", nonce()?));
        clipboard::write(&sentinel)?;
        self.visual.guard()?;
        self.require_owned_foreground()?;
        control.press().map_err(map_error)?;
        facts.response.copy_action = true;
        facts.stage = "response-readback";
        facts.response.clipboard_verified = self.wait_native_copy(marker, &sentinel)?;
        if !facts.response.clipboard_verified {
            return Err(Reason::ResponseMismatch);
        }
        facts.stage = "completed";
        Ok(())
    }

    fn wait_native_copy(&self, expected: &str, sentinel: &str) -> Result<bool, Reason> {
        let deadline = Instant::now() + WAIT;
        loop {
            self.visual.guard()?;
            let copied = clipboard::read()?;
            self.visual.guard()?;
            if exact_readback(&copied, expected, sentinel) {
                return Ok(true);
            }
            if copied.as_str() != sentinel || Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_copy_rejects_stale_partial_and_other_response_payloads() {
        assert!(exact_readback("reply-nonce", "reply-nonce", "sentinel"));
        for actual in [
            "sentinel",
            "reply-nonce old reply",
            "reply",
            "prompt reply-nonce",
            "",
        ] {
            assert!(!exact_readback(actual, "reply-nonce", "sentinel"));
        }
        assert!(!exact_readback("sentinel", "sentinel", "sentinel"));
    }
}
