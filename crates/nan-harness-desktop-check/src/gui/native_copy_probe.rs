//! Hosted-only native clipboard readback. No OCR or AX text reads.

use super::{ComposerErrorCategory, Gui, WAIT, clipboard, map_error, primary_modifier};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, ProbeResult, Reason};
use nan_harness_private_fs::open_private_new;
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
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
    provider_generation_count: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    navigation: &'static str,
    keyboard_transport: &'static str,
    response_method: &'static str,
    last_export_error: Option<String>,
    export_version: Option<String>,
    export_user_count: Option<usize>,
    export_assistant_text_count: Option<usize>,
    #[serde(skip)]
    expected_prompt: Zeroizing<String>,
    clipboard_readback: Option<&'static str>,
    clipboard_character_count: Option<usize>,
    experiment_only: bool,
    ocr_used: bool,
    ax_text_used: bool,
    stage: &'static str,
    substage: &'static str,
    guard_kind: Option<&'static str>,
    guard_category: Option<ComposerErrorCategory>,
    settle_observations: u8,
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

fn neutral_input(executable: &Path, mode: &str, prompt: &str) -> Result<(), Reason> {
    if !executable.is_absolute()
        || !executable.is_file()
        || prompt.len() > 4096
        || !matches!(
            mode,
            "type"
                | "new-thread"
                | "select-all"
                | "copy"
                | "copy-thread"
                | "paste"
                | "right"
                | "submit"
        )
        || (mode == "type" && prompt.is_empty())
        || (mode != "type" && !prompt.is_empty())
    {
        return Err(Reason::IsolationUnavailable);
    }
    let mut child = Command::new(executable)
        .arg(mode)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::ActionUnsupported)?;
    let outcome = std::thread::scope(|scope| {
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Reason::ActionUnsupported);
        };
        let writer = scope.spawn(move || stdin.write_all(prompt.as_bytes()));
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) if Instant::now() >= deadline => break None,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().map_err(|_| Reason::ActionUnsupported)?;
        if !status.is_some_and(|status| status.success()) {
            return Err(Reason::ActionUnsupported);
        }
        written.map_err(|_| Reason::ActionUnsupported)
    });
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ExportVerdict {
    verified: bool,
    version: Option<String>,
    user_count: usize,
    assistant_text_count: usize,
    error: Option<String>,
}

fn validate_export(prompt: &str, marker: &str, clipboard: &str) -> Result<ExportVerdict, Reason> {
    let parser =
        std::env::var_os("FEASIBILITY_ZED_EXPORT_PARSER").ok_or(Reason::IsolationUnavailable)?;
    let zstd = std::env::var_os("FEASIBILITY_ZED_ZSTD").ok_or(Reason::IsolationUnavailable)?;
    if !Path::new(&parser).is_absolute()
        || !Path::new(&parser).is_file()
        || !Path::new(&zstd).is_absolute()
        || !Path::new(&zstd).is_file()
    {
        return Err(Reason::IsolationUnavailable);
    }
    let request = Zeroizing::new(serde_json::to_vec(&serde_json::json!({"expectedPrompt": prompt, "expectedMarker": marker, "clipboard": clipboard})).map_err(|_| Reason::ActionUnsupported)?);
    let mut child = Command::new("python3")
        .arg(parser)
        .arg("--zstd")
        .arg(zstd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::ActionUnsupported)?;
    let outcome = std::thread::scope(|scope| {
        let mut stdin = child.stdin.take().ok_or(Reason::ActionUnsupported)?;
        let stdout = child.stdout.take().ok_or(Reason::ActionUnsupported)?;
        let writer = scope.spawn(|| {
            stdin.write_all(&request)?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        });
        let reader = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) if Instant::now() >= deadline => break None,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        writer
            .join()
            .map_err(|_| Reason::ActionUnsupported)?
            .map_err(|_| Reason::ActionUnsupported)?;
        let bytes = reader
            .join()
            .map_err(|_| Reason::ActionUnsupported)?
            .map_err(|_| Reason::ActionUnsupported)?;
        if !status.is_some_and(|status| status.success()) || bytes.len() > 4096 {
            return Err(Reason::ActionUnsupported);
        }
        let verdict: ExportVerdict =
            serde_json::from_slice(&bytes).map_err(|_| Reason::ActionUnsupported)?;
        if verdict.user_count > 128
            || verdict.assistant_text_count > 128
            || !matches!(
                verdict.error.as_deref(),
                None | Some(
                    "request" | "schema" | "user-mismatch" | "assistant-mismatch" | "decompression"
                )
            )
        {
            return Err(Reason::ActionUnsupported);
        }
        if verdict
            .version
            .as_deref()
            .is_some_and(|version| version != "1.0.0")
            || (verdict.verified
                && (verdict.version.as_deref() != Some("1.0.0")
                    || verdict.user_count != 1
                    || verdict.assistant_text_count != 1
                    || verdict.error.is_some()))
        {
            return Err(Reason::ActionUnsupported);
        }
        Ok(verdict)
    });
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

fn observe_settling(
    mut guard: impl FnMut() -> Result<(), Reason>,
    mut wait: impl FnMut(),
) -> Result<(), Reason> {
    for observation in 0..3 {
        if observation > 0 {
            wait();
        }
        guard()?;
    }
    Ok(())
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
            keyboard_transport: if std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER").is_some() {
                if matches!(
                    std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref(),
                    Ok("all" | "paste")
                ) {
                    if std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref() == Ok("paste")
                    {
                        "neutral-quartz-paste"
                    } else {
                        "neutral-quartz-all"
                    }
                } else {
                    "neutral-quartz"
                }
            } else {
                "xa11y"
            },
            experiment_only: true,
            ocr_used: false,
            ax_text_used: false,
            response_method: if std::env::var("FEASIBILITY_ZED_RESPONSE_METHOD").as_deref()
                == Ok("thread-export")
            {
                "thread-export"
            } else {
                "native-copy"
            },
            last_export_error: None,
            export_version: None,
            export_user_count: None,
            export_assistant_text_count: None,
            expected_prompt: Zeroizing::new(String::new()),
            clipboard_readback: None,
            clipboard_character_count: None,
            stage: "trust",
            substage: "trust-query",
            guard_kind: None,
            guard_category: None,
            settle_observations: 0,
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
        facts.response.provider_verified = provider.fixture_response_verified();
        facts.response.provider_generation_count =
            Some(provider.generation_count()).filter(|count| *count <= 4096);
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

    fn native_copy_guard(&self, facts: &mut Facts, substage: &'static str) -> Result<(), Reason> {
        facts.substage = substage;
        self.visual.guard_composer().map_err(|(reason, category)| {
            facts.guard_kind = Some("native-window");
            facts.guard_category = Some(category);
            reason
        })?;
        self.require_owned_foreground().inspect_err(|&reason| {
            facts.guard_kind = Some("direct-foreground");
            facts.guard_category = Some(match reason {
                Reason::FocusChanged => ComposerErrorCategory::FocusChanged,
                _ => ComposerErrorCategory::Other,
            });
        })
    }

    fn guarded_chord(
        &self,
        facts: &mut Facts,
        character: char,
        modifiers: &[xa11y::Key],
        before: &'static str,
        after: &'static str,
    ) -> Result<(), Reason> {
        self.native_copy_guard(facts, before)?;
        if matches!(
            std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref(),
            Ok("all" | "paste")
        ) {
            let mode = match character {
                'n' => "new-thread",
                'a' => "select-all",
                'c' => "copy",
                _ => return Err(Reason::ActionUnsupported),
            };
            Self::neutral_key(mode)?;
        } else {
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .chord(xa11y::Key::Char(character), modifiers)
                .map_err(map_error)?;
        }
        self.native_copy_guard(facts, after)
    }

    fn neutral_key(mode: &str) -> Result<(), Reason> {
        let executable =
            std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER").ok_or(Reason::IsolationUnavailable)?;
        neutral_input(Path::new(&executable), mode, "")
    }

    fn native_copy_key(mode: &str, key: xa11y::Key) -> Result<(), Reason> {
        if matches!(
            std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref(),
            Ok("all" | "paste")
        ) {
            Self::neutral_key(mode)
        } else {
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .press(key)
                .map_err(map_error)
        }
    }

    fn settle_native_copy_panel(&self, facts: &mut Facts) -> Result<(), Reason> {
        // CGEventPost does not acknowledge application delivery. Observe owned
        // foreground across a bounded interval; never recover a rejected guard.
        observe_settling(
            || {
                self.native_copy_guard(facts, "panel-settle")?;
                facts.settle_observations += 1;
                Ok(())
            },
            || std::thread::sleep(Duration::from_millis(100)),
        )
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
                self.native_copy_guard(facts, "trust-before")?;
                trust.press().map_err(map_error)?;
                trust.wait_hidden(WAIT).map_err(map_error)?;
                self.native_copy_guard(facts, "trust-after")?;
            }
            _ => return Err(Reason::SelectorNotMatched),
        }
        facts.stage = "panel";
        facts.substage = "panel-query";
        let panel = app.locator("*[name=\"Agent Panel\"]");
        let count = control_count(&panel)?;
        facts.panel_control_count = Some(count);
        if count > 1 {
            return Err(Reason::SelectorNotMatched);
        }
        // The private profile binds the global workspace NewThread handler,
        // which creates the draft and focuses its panel regardless of prior focus.
        self.guarded_chord(
            facts,
            'n',
            &[xa11y::Key::Ctrl, xa11y::Key::Alt],
            "new-thread-before",
            "new-thread-after",
        )?;
        self.settle_native_copy_panel(facts)?;
        self.native_copy_input(facts)?;
        facts.stage = "submit";
        // Collapse selection before the one source-bound MessageEditor Chat action.
        self.native_copy_guard(facts, "collapse-selection-before")?;
        Self::native_copy_key("right", xa11y::Key::ArrowRight)?;
        self.native_copy_guard(facts, "submit-before")?;
        Self::native_copy_key("submit", xa11y::Key::Enter)?;
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
        self.guarded_chord(
            facts,
            'a',
            &[primary_modifier()],
            "select-all-before",
            "select-all-after",
        )?;
        self.native_copy_guard(facts, "type-before")?;
        if std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref() == Ok("paste") {
            clipboard::write(&prompt)?;
            self.native_copy_guard(facts, "paste-before")?;
            Self::neutral_key("paste")?;
            self.native_copy_guard(facts, "paste-after")?;
            // Allow queued Paste to read its clipboard before replacing it.
            // This is one action, not a retry; exact independent copy gates send.
            std::thread::sleep(Duration::from_millis(100));
            self.native_copy_guard(facts, "paste-settle")?;
        } else if let Some(executable) = std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER") {
            neutral_input(Path::new(&executable), "type", &prompt)?;
        } else {
            xa11y::input_sim()
                .map_err(map_error)?
                .keyboard()
                .type_text(&prompt)
                .map_err(map_error)?;
        }
        facts.expected_prompt.clone_from(&prompt);
        facts.input.entered = true;
        self.native_copy_guard(facts, "type-after")?;
        let sentinel = Zeroizing::new(format!("clipboard-sentinel-{}", nonce()?));
        facts.substage = "input-sentinel-write";
        clipboard::write(&sentinel)?;
        self.guarded_chord(
            facts,
            'a',
            &[primary_modifier()],
            "copy-select-all-before",
            "copy-select-all-after",
        )?;
        self.guarded_chord(
            facts,
            'c',
            &[primary_modifier()],
            "input-copy-before",
            "input-copy-after",
        )?;
        facts.input.clipboard_verified = self.wait_native_copy(facts, &prompt, &sentinel)?;
        if facts.input.clipboard_verified {
            Ok(())
        } else {
            Err(Reason::InputMismatch)
        }
    }

    fn native_copy_response(&self, facts: &mut Facts, marker: &str) -> Result<(), Reason> {
        if facts.response_method == "thread-export" {
            return self.native_export_response(facts, marker);
        }
        facts.stage = "response-control";
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let control = app.locator(RESPONSE_COPY);
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            self.native_copy_guard(facts, "response-control-query")?;
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
        facts.substage = "response-sentinel-write";
        clipboard::write(&sentinel)?;
        self.native_copy_guard(facts, "response-copy-before")?;
        control.press().map_err(map_error)?;
        facts.response.copy_action = true;
        self.native_copy_guard(facts, "response-copy-after")?;
        facts.stage = "response-readback";
        facts.response.clipboard_verified = self.wait_native_copy(facts, marker, &sentinel)?;
        if !facts.response.clipboard_verified {
            return Err(Reason::ResponseMismatch);
        }
        facts.stage = "completed";
        facts.substage = "completed";
        Ok(())
    }

    fn native_export_response(&self, facts: &mut Facts, marker: &str) -> Result<(), Reason> {
        facts.stage = "response-readback";
        let export_deadline = Instant::now() + WAIT;
        for attempt in 0..5 {
            if Instant::now() >= export_deadline {
                break;
            }
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(200));
            }
            let sentinel = Zeroizing::new(format!("export-sentinel-{}", nonce()?));
            clipboard::write(&sentinel)?;
            self.native_copy_guard(facts, "export-copy-before")?;
            Self::neutral_key("copy-thread")?;
            facts.response.copy_action = true;
            self.native_copy_guard(facts, "export-copy-after")?;
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                self.native_copy_guard(facts, "export-read-before")?;
                let copied = clipboard::read()?;
                self.native_copy_guard(facts, "export-read-after")?;
                if copied.as_str() != sentinel.as_str() {
                    facts.substage = "export-parse";
                    let verdict = validate_export(&facts.expected_prompt, marker, &copied)?;
                    facts.last_export_error = verdict.error;
                    facts.export_version = verdict.version;
                    facts.export_user_count = Some(verdict.user_count);
                    facts.export_assistant_text_count = Some(verdict.assistant_text_count);
                    if verdict.verified {
                        facts.response.clipboard_verified = true;
                        facts.stage = "completed";
                        facts.substage = "completed";
                        return Ok(());
                    }
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Err(Reason::ResponseMismatch)
    }

    fn wait_native_copy(
        &self,
        facts: &mut Facts,
        expected: &str,
        sentinel: &str,
    ) -> Result<bool, Reason> {
        let deadline = Instant::now() + WAIT;
        loop {
            self.native_copy_guard(facts, "clipboard-read-before")?;
            let copied = clipboard::read()?;
            self.native_copy_guard(facts, "clipboard-read-after")?;
            facts.clipboard_character_count = Some(copied.chars().count());
            facts.clipboard_readback = Some(if exact_readback(&copied, expected, sentinel) {
                "exact"
            } else if copied.as_str() == sentinel {
                "sentinel"
            } else if copied.is_empty() {
                "empty"
            } else {
                "nonmatching"
            });
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
    fn neutral_transport_rejects_unbounded_or_relative_requests_before_spawn() {
        assert_eq!(
            neutral_input(Path::new("relative"), "type", "nonce"),
            Err(Reason::IsolationUnavailable)
        );
        assert_eq!(
            neutral_input(Path::new("/usr/bin/true"), "type", &"x".repeat(4097)),
            Err(Reason::IsolationUnavailable)
        );
        assert_eq!(
            neutral_input(Path::new("/usr/bin/true"), "type", ""),
            Err(Reason::IsolationUnavailable)
        );
        for (mode, payload) in [
            ("arbitrary", ""),
            ("copy", "synthetic"),
            ("submit", "synthetic"),
        ] {
            assert_eq!(
                neutral_input(Path::new("/usr/bin/true"), mode, payload),
                Err(Reason::IsolationUnavailable)
            );
        }
    }

    #[test]
    fn settling_stops_on_first_rejected_ownership_without_recovery() {
        let mut observations = 0;
        let mut waits = 0;
        let outcome = observe_settling(
            || {
                observations += 1;
                if observations == 2 {
                    Err(Reason::FocusChanged)
                } else {
                    Ok(())
                }
            },
            || waits += 1,
        );
        assert_eq!(outcome, Err(Reason::FocusChanged));
        assert_eq!((observations, waits), (2, 1));
        observations = 0;
        waits = 0;
        assert_eq!(
            observe_settling(
                || {
                    observations += 1;
                    Ok(())
                },
                || waits += 1
            ),
            Ok(())
        );
        assert_eq!((observations, waits), (3, 2));
    }

    #[test]
    fn input_oracle_rejects_uncopied_sentinel_and_previous_prompt() {
        let expected = "input-nonce-new";
        let sentinel = "input-copy-sentinel";
        assert!(!exact_readback(sentinel, expected, sentinel));
        assert!(!exact_readback("input-nonce-old", expected, sentinel));
        assert!(!exact_readback("", expected, sentinel));
        assert!(exact_readback(expected, expected, sentinel));
    }

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
