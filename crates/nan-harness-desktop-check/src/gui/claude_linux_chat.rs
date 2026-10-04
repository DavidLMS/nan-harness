//! Supervised source-bound Linux Chat input and assistant clipboard observations.
use super::{Gui, clipboard};
use crate::provider::ProviderGate;
use crate::report::Reason;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xa11y::Role;
use zeroize::Zeroizing;

pub(crate) fn policy() -> bool {
    [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Linux"),
        ("NANH_CLAUDE_LINUX_SOURCE_POLICY", "official-2.9939.4"),
        ("NANH_CLAUDE_LINUX_NATIVE_CHAT", "first-turn"),
        ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
    ]
    .into_iter()
    .all(|(key, value)| std::env::var(key).as_deref() == Ok(value))
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FailureBoundary {
    Request,
    Policy,
    NativeWindow,
    SourceOwner,
    Tree,
    State,
    Frame,
    FrameActive,
    FrameCount,
    FrameClient,
    Client,
    Mode,
    Focus,
    Input,
    Clipboard,
    Action,
    Response,
    Transport,
}
fn failure_boundary(facts: &Value) -> Option<FailureBoundary> {
    serde_json::from_value(facts.get("failureBoundary")?.clone()).ok()
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputShape {
    char_count: u16,
    only_line_breaks: bool,
    only_whitespace: bool,
    only_zero_width_markers: bool,
}
impl InputShape {
    fn valid(self) -> bool {
        (1..=4096).contains(&self.char_count)
            && (!self.only_line_breaks || self.only_whitespace)
            && (!self.only_zero_width_markers || !self.only_whitespace && !self.only_line_breaks)
    }
}
fn input_shape(facts: &Value) -> Option<InputShape> {
    let shape: InputShape = serde_json::from_value(facts.get("inputShape")?.clone()).ok()?;
    shape.valid().then_some(shape)
}

const FLAGS: [&str; 7] = [
    "inputVerified",
    "pasteAttempted",
    "sendAttempted",
    "sendForwarded",
    "responseVerified",
    "toolVerified",
    "recoveryVerified",
];
fn valid_binding(fields: &serde_json::Map<String, Value>) -> bool {
    if fields.len() != 6 {
        return false;
    }
    for key in ["editor", "frame"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 2
            || parts
                .iter()
                .any(|part| part.as_str().is_none_or(|s| s.is_empty() || s.len() > 1024))
        {
            return false;
        }
    }
    for key in ["editorIdentity", "frameIdentity"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 3
            || parts[0].as_u64().is_none_or(|role| role > 255)
            || parts[1..]
                .iter()
                .any(|part| part.as_str().is_none_or(|s| s.len() > 4096))
        {
            return false;
        }
    }
    for key in ["editorBounds", "frameBounds"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 4
            || parts.iter().any(|part| part.as_i64().is_none())
            || parts[2].as_i64().is_none_or(|n| n <= 0)
            || parts[3].as_i64().is_none_or(|n| n <= 0)
        {
            return false;
        }
    }
    true
}

fn decode(bytes: &[u8]) -> Option<(Value, Option<Value>)> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    if object.len() != 2 || !object.contains_key("facts") || !object.contains_key("binding") {
        return None;
    }
    let facts = value["facts"].as_object()?;
    if !(11..=13).contains(&facts.len())
        || facts.keys().any(|key| {
            !FLAGS.contains(&key.as_str())
                && ![
                    "schemaVersion",
                    "mechanism",
                    "diagnosticsOnly",
                    "stage",
                    "failureBoundary",
                    "inputShape",
                ]
                .contains(&key.as_str())
        })
        || value["facts"]["schemaVersion"] != 1
        || value["facts"]["diagnosticsOnly"] != true
        || value["facts"]["mechanism"] != "claude-linux-native-chat"
        || FLAGS.iter().any(|key| !value["facts"][*key].is_boolean())
        || value["facts"]["toolVerified"] != false
        || value["facts"]["recoveryVerified"] != false
        || ![
            "source",
            "focus",
            "paste",
            "readback",
            "send",
            "input-not-empty",
            "blocked",
            "action-uncertain",
            "deadline",
            "clipboard-cleanup",
            "sent",
            "response-pending",
            "response-mismatch",
            "copied",
        ]
        .contains(&value["facts"]["stage"].as_str()?)
        || value["facts"]["sendForwarded"] == true && value["facts"]["sendAttempted"] != true
        || value["facts"]["sendAttempted"] == true && value["facts"]["inputVerified"] != true
        || value["facts"]["responseVerified"] == true && value["facts"]["stage"] != "copied"
    {
        return None;
    }
    if facts.contains_key("inputShape")
        && (input_shape(&value["facts"]).is_none()
            || !["input-not-empty", "clipboard-cleanup"]
                .contains(&value["facts"]["stage"].as_str()?)
            || FLAGS.iter().any(|key| value["facts"][*key] != false))
    {
        return None;
    }
    if facts.contains_key("failureBoundary")
        && (failure_boundary(&value["facts"]).is_none()
            || ![
                "blocked",
                "action-uncertain",
                "deadline",
                "clipboard-cleanup",
                "input-not-empty",
                "response-mismatch",
            ]
            .contains(&value["facts"]["stage"].as_str()?))
    {
        return None;
    }
    let binding = match &value["binding"] {
        Value::Null => None,
        Value::Object(fields) if valid_binding(fields) => Some(value["binding"].clone()),
        _ => return None,
    };
    Some((value["facts"].clone(), binding))
}

fn supervise(
    driver: &Path,
    request: Zeroizing<String>,
    deadline: Instant,
) -> Result<(Value, Option<Value>), Reason> {
    let mut command = Command::new("/usr/bin/python3");
    command.arg(driver).env_clear();
    for key in [
        "DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "GITHUB_ACTIONS",
        "RUNNER_ENVIRONMENT",
        "RUNNER_OS",
        "NANH_CLAUDE_LINUX_SOURCE_POLICY",
        "NANH_CLAUDE_LINUX_NATIVE_CHAT",
        "NANH_DESKTOP_QUALIFICATION_MODE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::ActionUnsupported)?;
    let outcome = std::thread::scope(|scope| {
        let mut input = child.stdin.take()?;
        let output = child.stdout.take()?;
        let writer = scope.spawn(move || input.write_all(request.as_bytes()));
        let reader = scope.spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            output.take(65537).read_to_end(&mut bytes).ok()?;
            Some(bytes)
        });
        let status = loop {
            if Instant::now() >= deadline {
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ),
                Err(_) => break None,
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().ok()?.ok();
        let bytes = reader.join().ok()??;
        if written.is_none()
            || !status.is_some_and(|status| status.success())
            || bytes.len() > 65536
            || Instant::now() >= deadline
        {
            return None;
        }
        decode(&bytes)
    });
    if outcome.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome.ok_or(if Instant::now() >= deadline {
        Reason::Timeout
    } else {
        Reason::ActionUnsupported
    })
}

pub(crate) struct ClaudeLinuxChatSession<'a> {
    gui: &'a Gui,
    directory: PathBuf,
    driver: PathBuf,
    binding: Option<Value>,
    stage: String,
    failure_boundary: Option<FailureBoundary>,
    input_shape: Option<InputShape>,
    submitted: u8,
    verified: u8,
    copied: u8,
    input_in_flight: bool,
    copy_in_flight: bool,
}
impl Gui {
    pub(crate) fn claude_linux_chat_session(
        &self,
        directory: &Path,
    ) -> Result<ClaudeLinuxChatSession<'_>, Reason> {
        let driver = std::env::var_os("FEASIBILITY_CLAUDE_CHAT_DRIVER")
            .map(PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if self.kind != DesktopHarnessKind::Claude
            || !policy()
            || !driver.is_absolute()
            || driver.is_symlink()
            || !driver.is_file()
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(ClaudeLinuxChatSession {
            gui: self,
            directory: directory.to_owned(),
            driver,
            binding: None,
            stage: "source".into(),
            failure_boundary: None,
            input_shape: None,
            submitted: 0,
            verified: 0,
            copied: 0,
            input_in_flight: false,
            copy_in_flight: false,
        })
    }
}
impl ClaudeLinuxChatSession<'_> {
    fn operation(&mut self, mode: &str, value: &str, deadline: Instant) -> Result<Value, Reason> {
        self.stage = "blocked".into();
        self.failure_boundary = Some(FailureBoundary::NativeWindow);
        if Instant::now() >= deadline
            || !self.gui.visual.linux_passive_composer_guard_until(deadline)
        {
            return Err(Reason::FocusChanged);
        }
        self.failure_boundary = Some(FailureBoundary::SourceOwner);
        let data = &self.gui.app.as_ref().ok_or(Reason::ActionUnsupported)?.data;
        if data.role != Role::Application || data.pid != Some(self.gui.visual.pid()) {
            return Err(Reason::FocusChanged);
        }
        let bus = data
            .raw
            .get("bus_name")
            .and_then(Value::as_str)
            .ok_or(Reason::ActionUnsupported)?;
        let path = data.stable_id.as_deref().ok_or(Reason::ActionUnsupported)?;
        let ticks = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC)
            .map_err(|_| Reason::ActionUnsupported)?;
        let absolute = ticks.tv_sec().to_f64().ok_or(Reason::ActionUnsupported)?
            + ticks.tv_nsec().to_f64().ok_or(Reason::ActionUnsupported)? / 1_000_000_000.0
            + deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64();
        let mut request = self.gui.visual.linux_chat_window_request();
        request["bus"] = json!(bus);
        request["path"] = json!(path);
        request["checkerPid"] = json!(std::process::id());
        request["deadline"] = json!(absolute);
        request["mode"] = json!(mode);
        request["value"] = json!(value);
        request["binding"] = json!(self.binding);
        let payload = Zeroizing::new(request.to_string());
        self.failure_boundary = Some(FailureBoundary::Transport);
        let (facts, binding) = supervise(&self.driver, payload, deadline)?;
        self.failure_boundary = failure_boundary(&facts);
        self.input_shape = input_shape(&facts);
        self.stage = facts["stage"]
            .as_str()
            .ok_or(Reason::ActionUnsupported)?
            .to_owned();
        if !self.gui.visual.linux_passive_composer_guard_until(deadline) {
            self.stage = "blocked".into();
            self.failure_boundary = Some(FailureBoundary::NativeWindow);
            return Err(Reason::FocusChanged);
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        if let Some(binding) = binding {
            self.binding = Some(binding);
        }
        Ok(facts)
    }
    pub(crate) fn new_turn(&mut self, prompt: &str) -> Result<(), Reason> {
        if self.input_in_flight
            || self.copy_in_flight
            || self.copied != self.submitted
            || self.submitted >= 3
        {
            return Err(Reason::ActionUnsupported);
        }
        self.input_in_flight = true;
        let facts = self.operation("input", prompt, Instant::now() + Duration::from_secs(15))?;
        if facts["inputVerified"] == true {
            self.verified += 1;
        }
        if facts["sendForwarded"] != true {
            return Err(if self.stage == "deadline" {
                Reason::Timeout
            } else {
                Reason::ActionUnsupported
            });
        }
        self.submitted += 1;
        self.input_in_flight = false;
        Ok(())
    }
    pub(crate) fn wait_response(
        &mut self,
        marker: &str,
        timeout: Duration,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        if self.copy_in_flight || self.input_in_flight || self.copied >= self.submitted {
            return Err(Reason::ActionUnsupported);
        }
        let deadline = Instant::now() + timeout;
        loop {
            self.copy_in_flight = true;
            let facts = self.operation(
                "copy",
                marker,
                deadline.min(Instant::now() + Duration::from_secs(15)),
            )?;
            if facts["responseVerified"] == true {
                if clipboard::read()?.as_str() != marker || !gate.fixture_response_verified() {
                    return Err(Reason::ResponseMismatch);
                }
                self.copied += 1;
                self.copy_in_flight = false;
                return Ok(());
            }
            if self.stage != "response-pending" {
                return Err(Reason::ActionUnsupported);
            }
            self.copy_in_flight = false;
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
        _timeout: Duration,
        _gate: &ProviderGate,
    ) -> Result<(), Reason> {
        self.stage = "recovery-scope-unimplemented".into();
        self.failure_boundary = None;
        Err(Reason::ActionUnsupported)
    }
    pub(crate) fn retry_once(&mut self) -> Result<(), Reason> {
        Err(Reason::ActionUnsupported)
    }
    pub(crate) fn finish(
        mut self,
        _gate: &ProviderGate,
        outcome: Result<(), Reason>,
    ) -> Result<(), Reason> {
        let cleared =
            clipboard::write("").is_ok() && clipboard::read().is_ok_and(|value| value.is_empty());
        if !cleared {
            self.stage = "clipboard-cleanup".into();
            self.failure_boundary = Some(FailureBoundary::Clipboard);
        }
        let mut facts = json!({"schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,
            "stage":self.stage,"submittedTurns":self.submitted,"inputVerifiedTurns":self.verified,
            "copiedResponses":self.copied,"retryAttempted":false,"clipboardCleared":cleared});
        if let Some(boundary) = self.failure_boundary {
            facts["failureBoundary"] = json!(boundary);
        }
        if let Some(shape) = self.input_shape {
            facts["inputShape"] = json!(shape);
        }
        let recorded = open_private_new(&self.directory.join(format!(
            "claude-linux-native-chat-{}.json",
            self.gui.visual.pid()
        )))
        .and_then(|mut file| file.write_all(facts.to_string().as_bytes()));
        if !cleared {
            return Err(Reason::CleanupFailed);
        }
        recorded.map_err(|_| Reason::IsolationUnavailable)?;
        outcome
    }
}

#[cfg(test)]
mod input_shape_tests {
    use super::input_shape;
    use serde_json::json;

    #[test]
    fn input_shape_is_closed_bounded_and_diagnostic_only() {
        let valid = json!({"inputShape":{"charCount":1,"onlyLineBreaks":true,
            "onlyWhitespace":true,"onlyZeroWidthMarkers":false}});
        assert!(input_shape(&valid).is_some());
        for shape in [
            json!({"charCount":0,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":4097,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":true,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":1,"onlyLineBreaks":true,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":true,"onlyZeroWidthMarkers":true}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false,"value":"PRIVATE"}),
        ] {
            assert!(input_shape(&json!({"inputShape":shape})).is_none());
        }
    }
}
