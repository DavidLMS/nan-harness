//! Complete hosted deterministic scenarios using owned semantic UI adapters.

use super::{ProbeSpec, select_read_tool, visual_marker};
use crate::cli::{SessionMode, VerificationPolicy};
use crate::gui::{DomAction, DomPurpose, DomTurn, Gui, NativeClipboardSession};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, InputMode, ProbeResult, Reason, ResponseVerification};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::create_private_dir_all;
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) struct SemanticBackend {
    directory: PathBuf,
    kind: DesktopHarnessKind,
}

pub(super) struct SemanticScenario<'a> {
    pub inventory: &'a ScriptedProvider,
    pub gate: &'a ProviderGate,
    pub fixture: &'a Path,
    pub marker: &'a str,
}

impl SemanticBackend {
    pub(super) fn from_spec(spec: &ProbeSpec) -> Result<Option<Self>, Reason> {
        if spec.verification != VerificationPolicy::SemanticOnly {
            return Ok(None);
        }
        if spec.live || spec.session != SessionMode::GithubHosted || !spec.session.available() {
            return Err(Reason::IsolationUnavailable);
        }
        let supported = matches!(spec.kind, DesktopHarnessKind::Zed) && cfg!(target_os = "macos")
            || matches!(spec.kind, DesktopHarnessKind::Hermes) && cfg!(target_os = "linux");
        if !supported {
            return Err(Reason::ActionUnsupported);
        }
        let directory = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")
            .map(PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !directory.is_absolute() {
            return Err(Reason::IsolationUnavailable);
        }
        create_private_dir_all(&directory).map_err(|_| Reason::IsolationUnavailable)?;
        Ok(Some(Self {
            directory,
            kind: spec.kind,
        }))
    }

    pub(super) async fn run(
        &self,
        gui: &Gui,
        owner: Option<u32>,
        scenario: SemanticScenario<'_>,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        let mut ui = match self.kind {
            DesktopHarnessKind::Zed => {
                SemanticUi::Zed(Box::new(gui.native_clipboard_session(&self.directory)?))
            }
            DesktopHarnessKind::Hermes => SemanticUi::Hermes {
                gui,
                directory: &self.directory,
                owner: owner.ok_or(Reason::ApplicationExited)?,
            },
            _ => return Err(Reason::ActionUnsupported),
        };
        let outcome = complete_scenario(&mut ui, &scenario, result).await;
        ui.finish(scenario.gate, outcome)
    }
}

enum SemanticUi<'a> {
    Zed(Box<NativeClipboardSession<'a>>),
    Hermes {
        gui: &'a Gui,
        directory: &'a Path,
        owner: u32,
    },
}

impl SemanticUi<'_> {
    fn methods(&self) -> (InputMode, ResponseVerification) {
        match self {
            Self::Zed(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeThreadExport,
            ),
            Self::Hermes { .. } => (
                InputMode::RendererDomAndKeyboard,
                ResponseVerification::RendererDom,
            ),
        }
    }

    fn turn(
        &mut self,
        prompt: &str,
        marker: &str,
        purpose: DomPurpose,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        match self {
            Self::Zed(session) => {
                session.new_turn(prompt)?;
                match purpose {
                    DomPurpose::Response => session.wait_response(marker, Duration::from_secs(30)),
                    DomPurpose::Failure => session.wait_retry(Duration::from_secs(30)),
                }
            }
            Self::Hermes {
                gui,
                directory,
                owner,
            } => gui.qualify_dom_turn(
                directory,
                *owner,
                DomTurn {
                    prompt,
                    marker,
                    action: DomAction::Submit,
                    purpose,
                },
                gate,
            ),
        }
    }

    fn retry(&mut self, marker: &str, gate: &ProviderGate) -> Result<(), Reason> {
        match self {
            Self::Zed(session) => {
                session.retry_once()?;
                session.wait_response(marker, Duration::from_secs(30))
            }
            Self::Hermes {
                gui,
                directory,
                owner,
            } => gui.qualify_dom_turn(
                directory,
                *owner,
                DomTurn {
                    prompt: "Check the expected provider failure",
                    marker,
                    action: DomAction::Retry,
                    purpose: DomPurpose::Response,
                },
                gate,
            ),
        }
    }

    fn finish(self, gate: &ProviderGate, outcome: Result<(), Reason>) -> Result<(), Reason> {
        match self {
            Self::Zed(session) => session.finish(gate, outcome),
            Self::Hermes { .. } => outcome,
        }
    }
}

async fn complete_scenario(
    ui: &mut SemanticUi<'_>,
    scenario: &SemanticScenario<'_>,
    result: &mut ProbeResult,
) -> Result<(), Reason> {
    let SemanticScenario {
        inventory,
        gate,
        fixture,
        marker,
    } = *scenario;
    let (input, response) = ui.methods();
    gate.arm_fixture_response(marker)
        .map_err(|()| Reason::ProviderFailed)?;
    ui.turn("Check this connection", marker, DomPurpose::Response, gate)?;
    if !gate.fixture_response_verified() || !inventory.recording_bounded() {
        return Err(Reason::ProviderFailed);
    }
    result.record_input(input);
    result.record_response(response);
    result
        .steps
        .extend([CheckStep::InputSubmitted, CheckStep::ResponseVerified]);

    let (name, arguments) =
        select_read_tool(&inventory.chat_requests(), fixture).ok_or(Reason::ToolMismatch)?;
    let tool_marker = visual_marker("NAN CHECK TOOL")?;
    let tool = ScriptedProvider::start(ProviderScenario::tool(name, arguments, &tool_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    gate.use_upstream(tool.base_url());
    gate.reset_tool_verification();
    gate.arm_fixture_response(&tool_marker)
        .map_err(|()| Reason::ProviderFailed)?;
    ui.turn(
        "Read read-target.txt using your file tool.",
        &tool_marker,
        DomPurpose::Response,
        gate,
    )?;
    if !tool.completed()
        || !tool.recording_bounded()
        || !gate.tool_verified()
        || !gate.fixture_response_verified()
    {
        return Err(Reason::ToolMismatch);
    }
    result.steps.push(CheckStep::ToolVerified);

    gate.fail_recoverable_scenario(true);
    ui.turn(
        "Check the expected provider failure",
        "NAN_CHECK_EXPECTED_FAILURE",
        DomPurpose::Failure,
        gate,
    )?;
    if !gate.failure_observed() {
        return Err(Reason::ProviderFailed);
    }
    let recovered_marker = visual_marker("NAN CHECK RECOVERED")?;
    let recovered = ScriptedProvider::start(ProviderScenario::inventory(&recovered_marker))
        .await
        .map_err(|_| Reason::ProviderFailed)?;
    gate.use_upstream(recovered.base_url());
    gate.arm_fixture_response(&recovered_marker)
        .map_err(|()| Reason::ProviderFailed)?;
    gate.fail_recoverable_scenario(false);
    ui.retry(&recovered_marker, gate)?;
    if !gate.fixture_response_verified() || !recovered.recording_bounded() {
        return Err(Reason::ResponseMismatch);
    }
    result.steps.push(CheckStep::ErrorRecovered);
    Ok(())
}
