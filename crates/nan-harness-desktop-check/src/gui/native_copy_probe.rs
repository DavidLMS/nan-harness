//! Hosted-only clipboard input/response oracles and native control navigation.

use super::{ComposerErrorCategory, Gui, WAIT, clipboard, map_error, primary_modifier};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, ProbeResult, Reason};
use nan_harness_private_fs::open_private_new;
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const RESPONSE_COPY: &str = "button[name=\"Copy This Agent Response\"], button[description=\"Copy This Agent Response\"], menu_item[name=\"Copy This Agent Response\"]";
const RETRY_CONTROL: &str = "button[name=\"Retry\"], button[description=\"Retry\"], button[name=\"Retry Generation\"], button[description=\"Retry Generation\"]";

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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationFacts {
    activation_attempted: bool,
    activation_succeeded: bool,
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
    last_export_transport_error: Option<&'static str>,
    export_version: Option<String>,
    export_user_count: Option<usize>,
    export_assistant_text_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    export_resume_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    export_agent_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    export_total_assistant_text_count: Option<usize>,
    #[serde(skip)]
    expected_prompt: Zeroizing<String>,
    clipboard_readback: Option<&'static str>,
    clipboard_character_count: Option<usize>,
    #[serde(flatten)]
    activation: ActivationFacts,
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
    retry_control_count: Option<usize>,
    retry_selector: Option<&'static str>,
    retry_action_receipt: Option<&'static str>,
    retry_title_count: Option<usize>,
    retry_label_count: Option<usize>,
    retry_inventory_status: Option<&'static str>,
    retry_inventory_total: Option<usize>,
    retry_inventory_buttons: Option<usize>,
    retry_inventory_static_text: Option<usize>,
    retry_inventory_title_matches: Option<usize>,
    retry_inventory_generation_matches: Option<usize>,
    retry_inventory_retry_matches: Option<usize>,
    retry_candidate_count: Option<usize>,
    retry_tooltip_count: Option<usize>,
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivationRequest {
    pid: u32,
    x: i32,
    y: i32,
}

fn valid_activation_request(payload: &str) -> bool {
    serde_json::from_str::<ActivationRequest>(payload)
        .is_ok_and(|request| request.pid > 0 && request.pid <= i32::MAX.cast_unsigned())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PointerRequest {
    pid: u32,
    window: u64,
    x: i16,
    y: i16,
}

fn valid_pointer_request(payload: &str) -> bool {
    serde_json::from_str::<PointerRequest>(payload).is_ok_and(|request| {
        request.pid > 1
            && request.pid <= i32::MAX.cast_unsigned()
            && request.window > 0
            && u32::try_from(request.window).is_ok()
    })
}

fn pointer_transport_diagnostic(code: Option<i32>) {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
        return;
    };
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() || !directory.is_dir() || directory.is_symlink() {
        return;
    }
    let stage = match code {
        Some(0) => "dispatched",
        Some(2) => "invalid-request",
        Some(11) => "foreground-mismatch",
        Some(12) => "process-mismatch",
        Some(13) => "initial-query-failed",
        Some(14) => "movement-failed",
        Some(15) => "final-query-failed",
        Some(16) => "activation-failed",
        _ => "transport-failed",
    };
    let mut nonce = [0_u8; 8];
    if getrandom::fill(&mut nonce).is_ok()
        && let Ok(mut file) = open_private_new(
            &directory.join(format!("zed-pointer-{}.json", u64::from_le_bytes(nonce))),
        )
    {
        let value = serde_json::json!({"schemaVersion":1,"mechanism":"zed-pointer-transport",
            "diagnosticsOnly":true,"stage":stage});
        let _ = serde_json::to_writer(&mut file, &value);
    }
}

fn neutral_input(executable: &Path, mode: &str, prompt: &str) -> Result<(), Reason> {
    if !executable.is_absolute()
        || !executable.is_file()
        || prompt.len() > 4096
        || !matches!(
            mode,
            "type"
                | "activate-accessibility"
                | "new-thread"
                | "trust"
                | "select-all"
                | "copy"
                | "copy-thread"
                | "paste"
                | "right"
                | "submit"
                | "retry-click"
        )
        || (mode == "type" && prompt.is_empty())
        || (mode == "activate-accessibility" && !valid_activation_request(prompt))
        || (mode == "retry-click" && (!cfg!(target_os = "linux") || !valid_pointer_request(prompt)))
        || (!matches!(mode, "type" | "activate-accessibility" | "retry-click")
            && !prompt.is_empty())
    {
        return Err(Reason::IsolationUnavailable);
    }
    let mut command = Command::new(executable);
    command.env_clear();
    if let Some(script) = std::env::var_os("FEASIBILITY_ZED_INPUT_SCRIPT") {
        let script = PathBuf::from(script);
        if !script.is_absolute() || !script.is_file() {
            return Err(Reason::IsolationUnavailable);
        }
        command.arg(script);
    }
    command.arg(mode);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    #[cfg(target_os = "linux")]
    for key in ["DISPLAY", "XAUTHORITY"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
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
        if mode == "retry-click" {
            pointer_transport_diagnostic(status.and_then(|status| status.code()));
        }
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
    #[serde(default)]
    resume_count: usize,
    #[serde(default)]
    agent_count: usize,
    #[serde(default)]
    total_assistant_text_count: usize,
    error: Option<String>,
}

fn decode_export_verdict(
    bytes: &[u8],
    transport: &mut Option<&'static str>,
) -> Result<ExportVerdict, Reason> {
    *transport = Some("output-budget");
    if bytes.len() > 4096 {
        return Err(Reason::ActionUnsupported);
    }
    *transport = Some("json");
    let verdict: ExportVerdict =
        serde_json::from_slice(bytes).map_err(|_| Reason::ActionUnsupported)?;
    *transport = Some("verdict");
    if verdict.user_count > 128
        || verdict.assistant_text_count > 128
        || verdict.resume_count > 128
        || verdict.agent_count > 128
        || verdict.total_assistant_text_count > 16_384
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
    *transport = None;
    Ok(verdict)
}

fn validate_export(
    prompt: &str,
    marker: &str,
    clipboard: &str,
    transport: &mut Option<&'static str>,
) -> Result<ExportVerdict, Reason> {
    *transport = Some("configuration");
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
    *transport = Some("spawn");
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
        *transport = Some("pipes");
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
        let deadline = Instant::now() + Duration::from_secs(8);
        *transport = Some("wait");
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(_) => break None,
                Ok(None) if Instant::now() >= deadline => {
                    *transport = Some("timeout");
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if status.is_none() {
            return Err(Reason::ActionUnsupported);
        }
        *transport = Some("write");
        writer
            .join()
            .map_err(|_| Reason::ActionUnsupported)?
            .map_err(|_| Reason::ActionUnsupported)?;
        *transport = Some("read");
        let bytes = reader
            .join()
            .map_err(|_| Reason::ActionUnsupported)?
            .map_err(|_| Reason::ActionUnsupported)?;
        *transport = Some("exit");
        if !status.is_some_and(|status| status.success()) {
            return Err(Reason::ActionUnsupported);
        }
        decode_export_verdict(&bytes, transport)
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
    let count = match control.count() {
        Ok(count) => count,
        Err(xa11y::Error::SelectorNotMatched { .. }) => 0,
        Err(error) => return Err(map_error(error)),
    };
    if count > 4096 {
        return Err(Reason::ActionUnsupported);
    }
    Ok(count)
}

// AXPress can time out after the application has executed its callback.
// Never repeat that action: only the fresh export and provider oracle can
// establish recovery after this one ambiguous receipt.
fn retry_press_receipt(result: Result<(), xa11y::Error>) -> Result<&'static str, Reason> {
    match result {
        Ok(()) => Ok("acknowledged"),
        Err(xa11y::Error::Platform { code: -25204, .. }) => Ok("completion-unknown"),
        Err(error) => Err(map_error(error)),
    }
}

fn native_copy_facts() -> Facts {
    Facts {
        schema_version: 1,
        mechanism: "zed-native-copy",
        navigation: "private-keymap-new-thread",
        keyboard_transport: if std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER").is_some() {
            if matches!(
                std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref(),
                Ok("all" | "paste")
            ) {
                if std::env::var("FEASIBILITY_ZED_INPUT_DRIVER_MODE").as_deref() == Ok("paste") {
                    if cfg!(windows) {
                        "neutral-win32-paste"
                    } else if cfg!(target_os = "linux") {
                        "neutral-x11-paste"
                    } else {
                        "neutral-quartz-paste"
                    }
                } else {
                    "neutral-quartz-all"
                }
            } else {
                "neutral-quartz"
            }
        } else {
            "xa11y"
        },
        activation: ActivationFacts::default(),
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
        last_export_transport_error: None,
        export_version: None,
        export_user_count: None,
        export_assistant_text_count: None,
        export_resume_count: None,
        export_agent_count: None,
        export_total_assistant_text_count: None,
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
        retry_control_count: None,
        retry_selector: None,
        retry_action_receipt: None,
        retry_title_count: None,
        retry_label_count: None,
        retry_inventory_status: None,
        retry_inventory_total: None,
        retry_inventory_buttons: None,
        retry_inventory_static_text: None,
        retry_inventory_title_matches: None,
        retry_inventory_generation_matches: None,
        retry_inventory_retry_matches: None,
        retry_candidate_count: None,
        retry_tooltip_count: None,
        clipboard_cleanup: "not-run",
        input: InputFacts::default(),
        response: ResponseFacts::default(),
    }
}

fn finish_native_copy(
    directory: &Path,
    mut facts: Facts,
    provider: Option<&ProviderGate>,
    outcome: Result<(), Reason>,
) -> Result<(), Reason> {
    facts.blocker = outcome.err();
    facts.response.provider_verified =
        provider.is_some_and(ProviderGate::fixture_response_verified);
    facts.response.provider_generation_count = provider
        .map(ProviderGate::generation_count)
        .filter(|count| *count <= 4096);
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

fn retry_header_candidate(title: xa11y::Rect, button: &xa11y::Element) -> bool {
    button.pid.is_some()
        && button.bounds.is_some_and(|bounds| {
            bounds.x >= title.x
                && bounds.width > 0
                && bounds.height > 0
                && i64::from(bounds.y) < i64::from(title.y) + i64::from(title.height)
                && i64::from(bounds.y) + i64::from(bounds.height) > i64::from(title.y)
        })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RetryInventoryCounts {
    total: usize,
    buttons: usize,
    static_text: usize,
    title_matches: usize,
    generation_matches: usize,
    retry_matches: usize,
}

fn retry_inventory_counts<'a>(
    elements: impl IntoIterator<Item = &'a xa11y::ElementData>,
) -> RetryInventoryCounts {
    let mut counts = RetryInventoryCounts::default();
    for element in elements.into_iter().take(4096) {
        counts.total += 1;
        counts.buttons += usize::from(element.role == xa11y::Role::Button);
        counts.static_text += usize::from(element.role == xa11y::Role::StaticText);
        let matches = |expected| {
            element.name.as_deref() == Some(expected) || element.value.as_deref() == Some(expected)
        };
        counts.title_matches += usize::from(matches("An Error Happened"));
        counts.generation_matches += usize::from(matches("Retry Generation"));
        counts.retry_matches += usize::from(matches("Retry"));
    }
    counts
}

fn retry_label_parent(label: &xa11y::Element) -> Result<xa11y::Element, Reason> {
    let label_bounds = label.bounds.ok_or(Reason::ActionUnsupported)?;
    let pid = label.pid.ok_or(Reason::IsolationUnavailable)?;
    let mut parent = label.parent().map_err(map_error)?;
    for _ in 0..2 {
        let element = parent.ok_or(Reason::SelectorNotMatched)?;
        if element.pid != Some(pid) {
            return Err(Reason::IsolationUnavailable);
        }
        if element.role == xa11y::Role::Button {
            let bounds = element.bounds.ok_or(Reason::ActionUnsupported)?;
            if !rect_contains(bounds, label_bounds) {
                return Err(Reason::ActionUnsupported);
            }
            return Ok(element);
        }
        parent = element.parent().map_err(map_error)?;
    }
    Err(Reason::SelectorNotMatched)
}

fn rect_contains(outer: xa11y::Rect, inner: xa11y::Rect) -> bool {
    outer.width > 0
        && outer.height > 0
        && inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn same_retry_element(before: &xa11y::Element, after: &xa11y::Element) -> bool {
    before.pid.is_some()
        && before.pid == after.pid
        && before.bounds == after.bounds
        && before.stable_id == after.stable_id
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IconStage {
    Baseline,
    Templates,
    FirstCapture,
    SecondCapture,
    Matching,
    Completed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IconObservation {
    stage: IconStage,
    schema_version: u8,
    mechanism: &'static str,
    status: &'static str,
    reason: Option<Reason>,
    #[serde(flatten)]
    counts: super::native_icon_probe::IconDiagnostics,
}

fn record_icon_observation(directory: &Path, observation: &IconObservation) -> Result<(), Reason> {
    let bytes = serde_json::to_vec(observation).map_err(|_| Reason::IsolationUnavailable)?;
    let name = format!("icons-{}-{}.json", std::process::id(), nonce()?);
    open_private_new(&directory.join(name))
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .map_err(|_| Reason::IsolationUnavailable)
}

fn icon_guard_failure(reason: Reason) -> bool {
    matches!(
        reason,
        Reason::FocusChanged
            | Reason::WindowChanged
            | Reason::WindowOccluded
            | Reason::ApplicationExited
            | Reason::IsolationUnavailable
    )
}

pub(crate) struct NativeClipboardSession<'a> {
    gui: &'a Gui,
    directory: &'a Path,
    facts: Facts,
    retry_ready: bool,
    retry_element: Option<xa11y::Element>,
    icon_directory: Option<PathBuf>,
    icon_baseline: Option<super::native_icon_probe::PrivateIconFrame>,
}

impl NativeClipboardSession<'_> {
    pub(crate) fn new_turn(&mut self, prompt: &str) -> Result<(), Reason> {
        if prompt.is_empty() || prompt.len() > 1024 {
            return Err(Reason::InputMismatch);
        }
        self.retry_ready = false;
        self.retry_element = None;
        self.facts.input = InputFacts::default();
        self.facts.response = ResponseFacts::default();
        self.facts.settle_observations = 0;
        self.icon_baseline = None;
        self.gui.compose_native_copy_turn(&mut self.facts, prompt)?;
        self.capture_icon_baseline()?;
        self.gui.send_native_copy_turn(&mut self.facts)
    }

    fn record_icon_failure(&self, reason: Reason, stage: IconStage) -> Result<(), Reason> {
        record_icon_observation(
            self.directory,
            &IconObservation {
                stage,
                schema_version: 1,
                mechanism: "zed-native-icons",
                status: if matches!(
                    reason,
                    Reason::ActionUnsupported | Reason::SelectorNotMatched | Reason::BudgetExceeded
                ) {
                    "unsupported"
                } else {
                    "query-error"
                },
                reason: Some(reason),
                counts: super::native_icon_probe::IconDiagnostics::unsupported(),
            },
        )
    }

    fn capture_icon_baseline(&mut self) -> Result<(), Reason> {
        if self.icon_directory.is_none() {
            return Ok(());
        }
        self.gui
            .native_copy_guard(&mut self.facts, "icon-baseline-before")?;
        match self.gui.visual.native_icon_frame() {
            Ok(frame) => self.icon_baseline = Some(frame),
            Err(reason) => {
                self.record_icon_failure(reason, IconStage::Baseline)?;
                if icon_guard_failure(reason) {
                    return Err(reason);
                }
            }
        }
        self.gui
            .native_copy_guard(&mut self.facts, "icon-baseline-after")
    }

    fn observe_native_icons(&mut self) -> Result<(), Reason> {
        let Some(directory) = self.icon_directory.as_ref() else {
            return Ok(());
        };
        let Some(baseline) = self.icon_baseline.as_ref() else {
            return Ok(());
        };
        self.gui
            .native_copy_guard(&mut self.facts, "icon-observation-before")?;
        let mut stage = IconStage::Templates;
        let outcome = (|| {
            let templates = super::native_icon_probe::Templates::load(directory, baseline.scale())?;
            stage = IconStage::FirstCapture;
            let first = self.gui.visual.native_icon_frame()?;
            self.gui
                .native_copy_guard(&mut self.facts, "icon-observation-settle")?;
            std::thread::sleep(Duration::from_millis(200));
            self.gui
                .native_copy_guard(&mut self.facts, "icon-observation-after")?;
            stage = IconStage::SecondCapture;
            let second = self.gui.visual.native_icon_frame()?;
            stage = IconStage::Matching;
            super::native_icon_probe::observe(&templates, baseline, &first, &second)
        })();
        match outcome {
            Ok(counts) => record_icon_observation(
                self.directory,
                &IconObservation {
                    schema_version: 1,
                    mechanism: "zed-native-icons",
                    stage: IconStage::Completed,
                    status: "complete",
                    reason: None,
                    counts,
                },
            )?,
            Err(reason) => {
                self.record_icon_failure(reason, stage)?;
                if icon_guard_failure(reason) {
                    return Err(reason);
                }
            }
        }
        self.gui
            .native_copy_guard(&mut self.facts, "icon-observation-completed")
    }

    pub(crate) fn wait_response(&mut self, marker: &str, timeout: Duration) -> Result<(), Reason> {
        if !self.facts.input.submitted
            || marker.is_empty()
            || marker.len() > 2048
            || timeout > Duration::from_mins(2)
        {
            return Err(Reason::ResponseMismatch);
        }
        self.gui
            .native_export_response(&mut self.facts, marker, timeout)
    }

    pub(crate) fn wait_retry(&mut self, timeout: Duration) -> Result<(), Reason> {
        if !self.facts.input.submitted || timeout > Duration::from_mins(2) {
            return Err(Reason::ActionUnsupported);
        }
        let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let retry = app.locator(RETRY_CONTROL);
        self.facts.retry_selector = Some("retry-name-or-description");
        let deadline = Instant::now() + timeout;
        loop {
            self.gui
                .native_copy_guard(&mut self.facts, "retry-control-query")?;
            let count = control_count(&retry)?;
            self.facts.retry_control_count = Some(count);
            if count == 1 {
                retry.wait_visible(WAIT).map_err(map_error)?;
                self.retry_ready = true;
                return Ok(());
            }
            if count > 1 {
                return Err(Reason::SelectorNotMatched);
            }
            if let Some(button) = self.retry_text_button()? {
                self.facts.retry_selector = Some("retry-label");
                self.retry_element = Some(button);
                self.retry_ready = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.observe_retry_inventory()?;
                let outcome = self.discover_retry_tooltip();
                if outcome.is_err() {
                    self.observe_native_icons()?;
                }
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn observe_retry_inventory(&mut self) -> Result<(), Reason> {
        self.gui
            .native_copy_guard(&mut self.facts, "retry-inventory-before")?;
        let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        match app.locator("*").elements() {
            Ok(elements) => {
                self.facts.retry_inventory_status = Some(if elements.len() > 4096 {
                    "budget-exceeded"
                } else {
                    "complete"
                });
                let counts = retry_inventory_counts(elements.iter().map(xa11y::Element::data));
                self.facts.retry_inventory_total = Some(counts.total);
                self.facts.retry_inventory_buttons = Some(counts.buttons);
                self.facts.retry_inventory_static_text = Some(counts.static_text);
                self.facts.retry_inventory_title_matches = Some(counts.title_matches);
                self.facts.retry_inventory_generation_matches = Some(counts.generation_matches);
                self.facts.retry_inventory_retry_matches = Some(counts.retry_matches);
            }
            Err(_) => self.facts.retry_inventory_status = Some("query-error"),
        }
        self.gui
            .native_copy_guard(&mut self.facts, "retry-inventory-after")
    }

    fn retry_text_button(&mut self) -> Result<Option<xa11y::Element>, Reason> {
        self.facts.ax_text_used = true;
        self.gui
            .native_copy_guard(&mut self.facts, "retry-label-query")?;
        let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let labels = app
            .locator("static_text[value=\"Retry\"], static_text[name=\"Retry\"]")
            .elements()
            .map_err(map_error)?;
        self.facts.retry_label_count = Some(labels.len().min(4096));
        match labels.as_slice() {
            [] => Ok(None),
            [label] => {
                self.gui
                    .native_copy_guard(&mut self.facts, "retry-label-parent")?;
                let button = retry_label_parent(label)?;
                self.gui
                    .visual
                    .validate_native_bounds(button.bounds.ok_or(Reason::ActionUnsupported)?)?;
                Ok(Some(button))
            }
            _ => Err(Reason::SelectorNotMatched),
        }
    }

    fn discover_retry_tooltip(&mut self) -> Result<(), Reason> {
        let deadline = Instant::now() + Duration::from_secs(10);
        self.facts.retry_selector = Some("retry-tooltip");
        self.facts.ax_text_used = true;
        self.gui
            .native_copy_guard(&mut self.facts, "retry-title-query")?;
        let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let title = app
            .locator("static_text[value=\"An Error Happened\"]")
            .elements()
            .map_err(map_error)?;
        self.facts.retry_title_count = Some(title.len().min(4096));
        if title.len() != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        let title_bounds = title[0].bounds.ok_or(Reason::ActionUnsupported)?;
        self.gui.visual.validate_native_bounds(title_bounds)?;
        let title_pid = title[0].pid.ok_or(Reason::IsolationUnavailable)?;
        let buttons = app.locator("button").elements().map_err(map_error)?;
        if buttons.len() > 64 {
            return Err(Reason::SelectorNotMatched);
        }
        let candidates: Vec<_> = buttons
            .into_iter()
            .filter(|button| {
                button.pid == Some(title_pid) && retry_header_candidate(title_bounds, button)
            })
            .collect();
        self.facts.retry_candidate_count = Some(candidates.len());
        if candidates.is_empty() || candidates.len() > 8 {
            return Err(Reason::SelectorNotMatched);
        }
        let mut selected = None;
        for candidate in candidates {
            if Instant::now() >= deadline {
                return Err(Reason::SelectorNotMatched);
            }
            self.reset_retry_tooltip(deadline)?;
            self.gui
                .native_copy_guard(&mut self.facts, "retry-tooltip-hover")?;
            self.gui
                .visual
                .hover_native(candidate.bounds.ok_or(Reason::ActionUnsupported)?)?;
            self.gui
                .native_copy_guard(&mut self.facts, "retry-tooltip-query")?;
            if self.wait_retry_tooltip(true, deadline)? {
                if selected.is_some() {
                    return Err(Reason::SelectorNotMatched);
                }
                selected = Some(candidate);
            }
            self.reset_retry_tooltip(deadline)?;
        }
        if Instant::now() >= deadline {
            return Err(Reason::SelectorNotMatched);
        }
        self.retry_element = Some(selected.ok_or(Reason::SelectorNotMatched)?);
        self.retry_ready = true;
        Ok(())
    }

    fn wait_retry_tooltip(&mut self, present: bool, deadline: Instant) -> Result<bool, Reason> {
        let until = deadline.min(Instant::now() + Duration::from_secs(1));
        loop {
            self.gui
                .native_copy_guard(&mut self.facts, "retry-tooltip-query")?;
            let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
            if Instant::now() >= deadline {
                return Err(Reason::SelectorNotMatched);
            }
            let count = control_count(&app.locator("static_text[value=\"Retry Generation\"]"))?;
            self.facts.retry_tooltip_count = Some(count);
            if count > 1 {
                return Err(Reason::SelectorNotMatched);
            }
            if (count == 1) == present {
                return Ok(true);
            }
            if Instant::now() >= until {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn reset_retry_tooltip(&mut self, deadline: Instant) -> Result<(), Reason> {
        self.gui
            .native_copy_guard(&mut self.facts, "retry-tooltip-reset")?;
        self.gui.visual.neutral_pointer()?;
        self.gui
            .native_copy_guard(&mut self.facts, "retry-tooltip-clear")?;
        if !self.wait_retry_tooltip(false, deadline)? {
            return Err(Reason::SelectorNotMatched);
        }
        Ok(())
    }

    pub(crate) fn retry_once(&mut self) -> Result<(), Reason> {
        if !std::mem::take(&mut self.retry_ready) {
            return Err(Reason::ActionUnsupported);
        }
        if let Some(captured) = self.retry_element.take() {
            self.gui
                .native_copy_guard(&mut self.facts, "retry-revalidate")?;
            if self.facts.retry_selector == Some("retry-label") {
                let button = self
                    .retry_text_button()?
                    .ok_or(Reason::SelectorNotMatched)?;
                if !same_retry_element(&captured, &button) {
                    return Err(Reason::SelectorNotMatched);
                }
                self.gui
                    .native_copy_guard(&mut self.facts, "retry-before")?;
                self.facts.retry_action_receipt = Some(self.press_retry(&button)?);
                return self.gui.native_copy_guard(&mut self.facts, "retry-after");
            }
            let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
            let current = app.locator("button").elements().map_err(map_error)?;
            if current.len() > 64 {
                return Err(Reason::SelectorNotMatched);
            }
            let matches: Vec<_> = current
                .into_iter()
                .filter(|element| same_retry_element(&captured, element))
                .collect();
            if matches.len() != 1 {
                return Err(Reason::SelectorNotMatched);
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            self.reset_retry_tooltip(deadline)?;
            self.gui
                .native_copy_guard(&mut self.facts, "retry-tooltip-hover")?;
            self.gui
                .visual
                .hover_native(matches[0].bounds.ok_or(Reason::ActionUnsupported)?)?;
            if !self.wait_retry_tooltip(true, deadline)? {
                return Err(Reason::SelectorNotMatched);
            }
            self.gui
                .native_copy_guard(&mut self.facts, "retry-before")?;
            self.facts.retry_action_receipt = Some(self.press_retry(&matches[0])?);
            return self.gui.native_copy_guard(&mut self.facts, "retry-after");
        }
        let app = self.gui.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let retry = app.locator(RETRY_CONTROL);
        let count = control_count(&retry)?;
        self.facts.retry_control_count = Some(count);
        if count != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        self.gui
            .native_copy_guard(&mut self.facts, "retry-before")?;
        let elements = retry.elements().map_err(map_error)?;
        if elements.len() != 1 {
            return Err(Reason::SelectorNotMatched);
        }
        self.facts.retry_action_receipt = Some(self.press_retry(&elements[0])?);
        self.gui.native_copy_guard(&mut self.facts, "retry-after")
    }

    fn press_retry(&self, button: &xa11y::Element) -> Result<&'static str, Reason> {
        if cfg!(target_os = "macos") {
            return retry_press_receipt(button.press());
        }
        #[cfg(target_os = "linux")]
        {
            let bounds = button.bounds.ok_or(Reason::ActionUnsupported)?;
            let (pid, window, point) = self.gui.visual.native_pointer_target(bounds)?;
            let request = serde_json::to_string(&PointerRequest {
                pid,
                window,
                x: point.x.try_into().map_err(|_| Reason::ActionUnsupported)?,
                y: point.y.try_into().map_err(|_| Reason::ActionUnsupported)?,
            })
            .map_err(|_| Reason::IsolationUnavailable)?;
            let executable = std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER")
                .ok_or(Reason::IsolationUnavailable)?;
            neutral_input(Path::new(&executable), "retry-click", &request)?;
            Ok("native-pointer-dispatched")
        }
        #[cfg(not(target_os = "linux"))]
        {
            // Windows uses an ordinary pointer as the primary Retry action.
            // The named control and its owned native bounds are revalidated first;
            // an uncertain dispatch never triggers another action or a fallback.
            let bounds = button.bounds.ok_or(Reason::ActionUnsupported)?;
            self.gui.visual.click_native(bounds)?;
            Ok("native-pointer-dispatched")
        }
    }

    pub(crate) fn finish(
        self,
        provider: &ProviderGate,
        outcome: Result<(), Reason>,
    ) -> Result<(), Reason> {
        finish_native_copy(self.directory, self.facts, Some(provider), outcome)
    }
}

impl Gui {
    pub(crate) fn native_clipboard_session<'a>(
        &'a self,
        directory: &'a Path,
    ) -> Result<NativeClipboardSession<'a>, Reason> {
        if !cfg!(any(target_os = "macos", target_os = "linux", windows))
            || self.kind != nan_harness_core::DesktopHarnessKind::Zed
        {
            return Err(Reason::ActionUnsupported);
        }
        let mut facts = native_copy_facts();
        facts.response_method = "thread-export";
        facts.experiment_only = false;
        if let Err(reason) = self
            .activate_native_accessibility(&mut facts)
            .and_then(|()| self.prepare_native_copy(&mut facts))
        {
            finish_native_copy(directory, facts, None, Err(reason))?;
            return Err(reason);
        }
        Ok(NativeClipboardSession {
            gui: self,
            directory,
            facts,
            retry_ready: false,
            retry_element: None,
            icon_directory: std::env::var_os("NANH_ZED_ICON_TEMPLATES")
                .map(PathBuf::from)
                .filter(|path| path.is_dir()),
            icon_baseline: None,
        })
    }

    pub(crate) fn probe_native_copy(
        &self,
        directory: &Path,
        marker: &str,
        result: &mut ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        let mut facts = native_copy_facts();
        let outcome = self.run_native_copy(&mut facts, marker, result);
        finish_native_copy(directory, facts, Some(provider), outcome)
    }

    fn activate_native_accessibility(&self, facts: &mut Facts) -> Result<(), Reason> {
        self.native_copy_guard(facts, "activation-before")?;
        if cfg!(target_os = "macos")
            && let Some(executable) = std::env::var_os("FEASIBILITY_ZED_INPUT_DRIVER")
        {
            let (pid, point) = self.visual.accessibility_activation_target()?;
            let request = serde_json::to_string(&ActivationRequest {
                pid,
                x: point.x,
                y: point.y,
            })
            .map_err(|_| Reason::IsolationUnavailable)?;
            facts.activation.activation_attempted = true;
            facts.activation.activation_succeeded =
                neutral_input(Path::new(&executable), "activate-accessibility", &request).is_ok();
        }
        self.native_copy_guard(facts, "activation-after")
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
        self.prepare_native_copy(facts)?;
        let prompt = Zeroizing::new(format!(
            "Check this connection. Read read-target.txt. Input nonce {}.",
            nonce()?
        ));
        self.new_native_copy_turn(facts, &prompt)?;
        result.steps.push(CheckStep::InputSubmitted);
        self.native_copy_response(facts, marker)
    }

    fn prepare_native_copy(&self, facts: &mut Facts) -> Result<(), Reason> {
        let app = self.app.as_ref().ok_or(Reason::SelectorNotMatched)?;
        let trust = app.locator("button[name=\"Trust and Continue\"]");
        let mut count = 0;
        // The owned window can precede the asynchronous workspace trust modal.
        for observation in 0..30 {
            count = control_count(&trust)?;
            if count != 0 || observation == 29 {
                break;
            }
            self.native_copy_guard(facts, "trust-query")?;
            std::thread::sleep(Duration::from_millis(100));
        }
        facts.trust_control_count = Some(count);
        match count {
            0 => {}
            1 => {
                self.native_copy_guard(facts, "trust-before")?;
                if cfg!(target_os = "macos") {
                    trust.press().map_err(map_error)?;
                } else {
                    // This private binding is scoped to the frozen SecurityModal;
                    // it invokes its normal Confirm action once, with no retry.
                    Self::neutral_key("trust")?;
                }
                let deadline = Instant::now() + WAIT;
                while control_count(&trust)? != 0 {
                    if Instant::now() >= deadline {
                        return Err(Reason::Timeout);
                    }
                    self.native_copy_guard(facts, "trust-after")?;
                    std::thread::sleep(Duration::from_millis(50));
                }
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
        Ok(())
    }

    fn new_native_copy_turn(&self, facts: &mut Facts, prompt: &str) -> Result<(), Reason> {
        self.compose_native_copy_turn(facts, prompt)?;
        self.send_native_copy_turn(facts)
    }

    fn compose_native_copy_turn(&self, facts: &mut Facts, prompt: &str) -> Result<(), Reason> {
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
        self.native_copy_input(facts, prompt)
    }

    fn send_native_copy_turn(&self, facts: &mut Facts) -> Result<(), Reason> {
        facts.stage = "submit";
        // Collapse selection before the one source-bound MessageEditor Chat action.
        self.native_copy_guard(facts, "collapse-selection-before")?;
        Self::native_copy_key("right", xa11y::Key::ArrowRight)?;
        self.native_copy_guard(facts, "submit-before")?;
        Self::native_copy_key("submit", xa11y::Key::Enter)?;
        facts.input.submitted = true;
        Ok(())
    }

    fn native_copy_input(&self, facts: &mut Facts, prompt: &str) -> Result<(), Reason> {
        facts.stage = "input";
        let prompt = Zeroizing::new(prompt.to_owned());
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
            return self.native_export_response(facts, marker, WAIT);
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

    fn native_export_response(
        &self,
        facts: &mut Facts,
        marker: &str,
        timeout: Duration,
    ) -> Result<(), Reason> {
        facts.stage = "response-readback";
        let export_deadline = Instant::now() + timeout;
        // Recovery can finish at the provider before the renderer publishes its
        // resumed thread. Repeat only read-only exports within the caller deadline.
        for attempt in 0..128 {
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
                    let verdict = validate_export(
                        &facts.expected_prompt,
                        marker,
                        &copied,
                        &mut facts.last_export_transport_error,
                    )?;
                    facts.last_export_error = verdict.error;
                    facts.export_version = verdict.version;
                    facts.export_user_count = Some(verdict.user_count);
                    facts.export_assistant_text_count = Some(verdict.assistant_text_count);
                    facts.export_resume_count = Some(verdict.resume_count);
                    facts.export_agent_count = Some(verdict.agent_count);
                    facts.export_total_assistant_text_count =
                        Some(verdict.total_assistant_text_count);
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
    fn export_transport_distinguishes_budget_json_and_untrusted_verdict() {
        let mut category = None;
        assert!(decode_export_verdict(&vec![b'x'; 4097], &mut category).is_err());
        assert_eq!(category, Some("output-budget"));
        assert!(decode_export_verdict(b"not-json", &mut category).is_err());
        assert_eq!(category, Some("json"));
        let inconsistent = br#"{"verified":true,"version":"1.0.0","userCount":1,"assistantTextCount":2,"error":null}"#;
        assert!(decode_export_verdict(inconsistent, &mut category).is_err());
        assert_eq!(category, Some("verdict"));
        let valid = br#"{"verified":false,"version":"1.0.0","userCount":1,"assistantTextCount":0,"error":"assistant-mismatch"}"#;
        assert!(
            !decode_export_verdict(valid, &mut category)
                .unwrap()
                .verified
        );
        assert_eq!(category, None);
    }

    #[test]
    fn neutral_transport_rejects_unbounded_or_relative_requests_before_spawn() {
        assert!(valid_pointer_request(
            r#"{"pid":20,"window":40,"x":100,"y":200}"#
        ));
        for request in [
            r#"{"pid":1,"window":40,"x":100,"y":200}"#,
            r#"{"pid":20,"window":0,"x":100,"y":200}"#,
            r#"{"pid":20,"window":40,"x":32768,"y":200}"#,
            r#"{"pid":20,"window":40,"x":100,"y":200,"command":"PRIVATE"}"#,
        ] {
            assert!(!valid_pointer_request(request));
        }
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
    fn inventory_reduces_role_independent_matches_once_and_bounds_observations() {
        let data = |role, name: Option<&str>, value: Option<&str>| xa11y::ElementData {
            role,
            name: name.map(str::to_owned),
            value: value.map(str::to_owned),
            description: None,
            bounds: None,
            actions: Vec::new(),
            states: xa11y::StateSet::default(),
            numeric_value: None,
            min_value: None,
            max_value: None,
            stable_id: None,
            pid: None,
            raw: std::collections::HashMap::default(),
            handle: 0,
        };
        let elements = [
            data(xa11y::Role::Button, Some("Retry"), Some("Retry")),
            data(xa11y::Role::Group, None, Some("An Error Happened")),
            data(xa11y::Role::StaticText, Some("Retry Generation"), None),
            data(xa11y::Role::StaticText, Some("prefix Retry"), None),
        ];
        assert_eq!(
            retry_inventory_counts(&elements),
            RetryInventoryCounts {
                total: 4,
                buttons: 1,
                static_text: 2,
                title_matches: 1,
                generation_matches: 1,
                retry_matches: 1,
            }
        );
        let repeated = std::iter::repeat_n(&elements[0], 4097);
        assert_eq!(
            retry_inventory_counts(repeated),
            RetryInventoryCounts {
                total: 4096,
                buttons: 4096,
                retry_matches: 4096,
                ..Default::default()
            }
        );
    }

    #[test]
    fn activation_requests_reject_foreign_shapes_and_invalid_process_ids() {
        assert!(valid_activation_request(r#"{"pid":123,"x":-40,"y":20}"#));
        for request in [
            r#"{"pid":0,"x":0,"y":0}"#,
            r#"{"pid":4294967295,"x":0,"y":0}"#,
            r#"{"pid":123,"x":0,"y":0,"text":"private"}"#,
            r#"{"pid":123,"x":1.5,"y":0}"#,
            "",
        ] {
            assert!(!valid_activation_request(request));
        }
    }

    #[test]
    fn retry_label_requires_positive_contained_geometry() {
        let outer = xa11y::Rect {
            x: -20,
            y: 10,
            width: 80,
            height: 30,
        };
        let inner = xa11y::Rect {
            x: -10,
            y: 15,
            width: 40,
            height: 10,
        };
        assert!(rect_contains(outer, inner));
        for invalid in [
            xa11y::Rect { width: 0, ..inner },
            xa11y::Rect { x: -21, ..inner },
            xa11y::Rect { y: 35, ..inner },
            xa11y::Rect {
                width: u32::MAX,
                ..inner
            },
        ] {
            assert!(!rect_contains(outer, invalid));
        }
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
    fn retry_receipt_allows_observation_only_for_exact_ax_completion_timeout() {
        assert_eq!(retry_press_receipt(Ok(())), Ok("acknowledged"));
        assert_eq!(
            retry_press_receipt(Err(xa11y::Error::Platform {
                code: -25204,
                message: String::new(),
            })),
            Ok("completion-unknown")
        );
        assert_eq!(
            retry_press_receipt(Err(xa11y::Error::Platform {
                code: -25202,
                message: String::new(),
            })),
            Err(Reason::DesktopUnavailable)
        );
        assert_eq!(
            retry_press_receipt(Err(xa11y::Error::PermissionDenied {
                instructions: String::new(),
            })),
            Err(Reason::PermissionRequired)
        );
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
