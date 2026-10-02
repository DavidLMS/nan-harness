//! Complete hosted deterministic scenarios using owned semantic UI adapters.

use super::{ProbeSpec, select_read_tool, visual_marker};
use crate::cli::{SessionMode, VerificationPolicy};
use crate::gui::{DomAction, DomPurpose, DomTurn, Gui, NativeClipboardSession, RendererSession};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, InputMode, ProbeResult, Reason, ResponseVerification};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::{create_private_dir_all, open_private_new};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use serde::Serialize;
use std::io::Write as _;
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
        if spec.kind == DesktopHarnessKind::Hermes {
            super::hermes_readiness::requested()?;
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

    pub(super) fn uses_renderer(&self) -> bool {
        self.kind != DesktopHarnessKind::Zed
            && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
    }

    pub(super) async fn run_renderer(
        &self,
        process: &mut crate::process::ProbeProcess,
        scenario: SemanticScenario<'_>,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        let mut session = RendererSession::new(process, &self.directory)?;
        if self.kind != DesktopHarnessKind::Hermes {
            session.inventory()?;
            result.steps.push(CheckStep::Launched);
            // A read-only inventory never qualifies the application.
            return Err(Reason::ActionUnsupported);
        }
        session.prepare_hermes_profile(
            scenario
                .fixture
                .parent()
                .ok_or(Reason::IsolationUnavailable)?,
        )?;
        // Endpoint readiness proves the launcher has applied the fresh profile
        // and spawned its GUI; no user input occurs before the retry policy.
        session.turn(
            DomTurn {
                prompt: "Check this connection",
                marker: scenario.marker,
                action: DomAction::Ready,
                purpose: DomPurpose::Response,
            },
            scenario.gate,
        )?;
        super::hermes_policy::prepare(
            scenario
                .fixture
                .parent()
                .ok_or(Reason::IsolationUnavailable)?,
            &self.directory,
        )?;
        let mut ui = SemanticUi::Renderer(session);
        complete_scenario(&mut ui, &scenario, &self.directory, result).await
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
            DesktopHarnessKind::Hermes => return Err(Reason::IsolationUnavailable),
            DesktopHarnessKind::ChatGpt | DesktopHarnessKind::Claude | DesktopHarnessKind::Pen => {
                return gui
                    .inventory_renderer(&self.directory, owner.ok_or(Reason::ApplicationExited)?);
            }
        };
        let outcome = complete_scenario(&mut ui, &scenario, &self.directory, result).await;
        ui.finish(scenario.gate, outcome)
    }
}

enum SemanticUi<'a> {
    Zed(Box<NativeClipboardSession<'a>>),
    Renderer(RendererSession<'a>),
}

impl SemanticUi<'_> {
    fn methods(&self) -> (InputMode, ResponseVerification) {
        match self {
            Self::Zed(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeThreadExport,
            ),
            Self::Renderer(_) => (
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
            Self::Renderer(session) => session.turn(
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

    fn inject_failure(&self, gate: &ProviderGate, directory: &Path) -> Result<(), Reason> {
        // Zed exposes Retry for a rejected request without scheduling automatic
        // retries. This isolates the explicit UI recovery contract from backoff.
        let status = match self {
            Self::Zed(_) => {
                gate.fail_next_scenario(true);
                400
            }
            Self::Renderer(_) => {
                gate.fail_recoverable_scenario(true);
                503
            }
        };
        let policy = serde_json::json!({"schemaVersion":1, "mechanism":"semantic-failure-policy",
            "failureStatus":status, "recoveryAction":"explicit-ui-retry"});
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
        open_private_new(
            &directory.join(format!("failure-policy-{}.json", u64::from_le_bytes(nonce))),
        )
        .and_then(|mut file| file.write_all(policy.to_string().as_bytes()))
        .map_err(|_| Reason::IsolationUnavailable)
    }

    fn retry(&mut self, marker: &str, gate: &ProviderGate) -> Result<(), Reason> {
        match self {
            Self::Zed(session) => {
                session.retry_once()?;
                session.wait_response(marker, Duration::from_secs(30))
            }
            Self::Renderer(session) => session.turn(
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
            Self::Renderer(_) => outcome,
        }
    }
}

async fn complete_scenario(
    ui: &mut SemanticUi<'_>,
    scenario: &SemanticScenario<'_>,
    directory: &Path,
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
    if !result.steps.contains(&CheckStep::Launched) {
        result.steps.push(CheckStep::Launched);
    }
    if !gate.fixture_response_verified() || !inventory.recording_bounded() {
        return Err(Reason::ProviderFailed);
    }
    result.record_input(input);
    result.record_response(response);
    result
        .steps
        .extend([CheckStep::InputSubmitted, CheckStep::ResponseVerified]);

    let requests = inventory.chat_requests();
    let selected = select_read_tool(&requests, fixture);
    record_inventory(directory, &requests, selected.is_some())?;
    let (name, arguments) = selected.ok_or(Reason::ToolMismatch)?;
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
    record_provider_oracle(directory, "tool", &tool, gate)?;
    if !tool.completed()
        || !tool.recording_bounded()
        || !gate.tool_verified()
        || !gate.fixture_response_verified()
    {
        return Err(Reason::ToolMismatch);
    }
    result.steps.push(CheckStep::ToolVerified);

    gate.arm_fixture_response("NAN_CHECK_EXPECTED_FAILURE")
        .map_err(|()| Reason::ProviderFailed)?;
    ui.inject_failure(gate, directory)?;
    let failure = ui.turn(
        "Check the expected provider failure",
        "NAN_CHECK_EXPECTED_FAILURE",
        DomPurpose::Failure,
        gate,
    );
    record_provider_oracle(directory, "failure", &tool, gate)?;
    failure?;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolOracleFacts {
    tool_completed: bool,
    tool_recording_bounded: bool,
    tool_verified: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderOracleFacts {
    schema_version: u8,
    mechanism: &'static str,
    stage: &'static str,
    #[serde(flatten)]
    tool: ToolOracleFacts,
    fixture_response_verified: bool,
    failure_observed: bool,
}

fn record_provider_oracle(
    directory: &Path,
    stage: &'static str,
    tool: &ScriptedProvider,
    gate: &ProviderGate,
) -> Result<(), Reason> {
    let facts = ProviderOracleFacts {
        schema_version: 1,
        mechanism: "semantic-provider-oracle",
        stage,
        tool: ToolOracleFacts {
            tool_completed: tool.completed(),
            tool_recording_bounded: tool.recording_bounded(),
            tool_verified: gate.tool_verified(),
        },
        fixture_response_verified: gate.fixture_response_verified(),
        failure_observed: gate.failure_observed(),
    };
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
    let path = directory.join(format!("provider-{}.json", u64::from_le_bytes(nonce)));
    let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
    open_private_new(&path)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .map_err(|_| Reason::IsolationUnavailable)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventoryFacts {
    schema_version: u8,
    mechanism: &'static str,
    request_count: usize,
    tool_count: usize,
    known_read_tool_count: usize,
    read_tool_selected: bool,
}

fn record_inventory(
    directory: &Path,
    requests: &[serde_json::Value],
    selected: bool,
) -> Result<(), Reason> {
    let tools: Vec<_> = requests
        .iter()
        .filter_map(|request| request.get("tools")?.as_array())
        .flatten()
        .collect();
    let facts = InventoryFacts {
        schema_version: 1,
        mechanism: "semantic-inventory",
        request_count: requests.len(),
        tool_count: tools.len(),
        read_tool_selected: selected,
        known_read_tool_count: tools
            .iter()
            .filter(|tool| {
                matches!(
                    tool.pointer("/function/name")
                        .and_then(serde_json::Value::as_str),
                    Some("Read" | "read_file" | "read_files" | "exec_command")
                )
            })
            .count(),
    };
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
    let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
    open_private_new(&directory.join(format!("inventory-{}.json", u64::from_le_bytes(nonce))))
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .map_err(|_| Reason::IsolationUnavailable)
}
