//! Complete hosted deterministic scenarios using owned semantic UI adapters.

use super::{OwnedReadFixtureSelection, ProbeSpec, select_semantic_read_tool, semantic_marker};
use crate::cli::{SessionMode, VerificationPolicy};
#[cfg(target_os = "linux")]
use crate::gui::ClaudeLinuxChatSession;
#[cfg(target_os = "macos")]
use crate::gui::ClaudeNativeChatSession;
#[cfg(windows)]
use crate::gui::ClaudeWindowsChatSession;
use crate::gui::{
    CodexDomSession, ComposerFailure, DomAction, DomPurpose, DomTurn, Gui, NativeClipboardSession,
    RendererSession,
};
use crate::provider::{ProviderGate, SelectedTool, ToolResultObservation};
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
    workspace: PathBuf,
}

pub(super) struct SemanticScenario<'a> {
    pub inventory: &'a ScriptedProvider,
    pub gate: &'a ProviderGate,
    pub fixture: &'a Path,
    pub marker: &'a str,
}

pub(super) struct NativeInputAuthority<'a> {
    pub native_roots: Option<&'a super::NativeRoots>,
    #[cfg(windows)]
    pub windows_profile: Option<&'a super::FreshClaudeWindowsProfile>,
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
            workspace: spec.workspace.clone(),
        }))
    }

    pub(super) fn uses_renderer(&self) -> bool {
        #[cfg(target_os = "linux")]
        if self.kind == DesktopHarnessKind::Claude && crate::gui::claude_linux_chat_policy() {
            return false;
        }
        self.kind != DesktopHarnessKind::Zed
            && std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
    }

    pub(super) async fn run_renderer(
        &self,
        process: &mut crate::process::ProbeProcess,
        scenario: SemanticScenario<'_>,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        if self.kind == DesktopHarnessKind::ChatGpt {
            {
                let mut inventory = RendererSession::new(process, &self.directory)?;
                inventory.inventory_with_workspace(&self.workspace)?;
            }
            let mut session = CodexDomSession::new(process, &self.directory)?;
            session.turn(
                DomTurn {
                    prompt: "Check this connection",
                    marker: scenario.marker,
                    action: DomAction::Ready,
                    purpose: DomPurpose::Response,
                },
                scenario.gate,
            )?;
            let mut ui = SemanticUi::Codex(session);
            return complete_scenario(&mut ui, &scenario, &self.directory, result).await;
        }
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
        composer_observations: &mut Vec<ComposerFailure>,
        authority: NativeInputAuthority<'_>,
    ) -> Result<(), Reason> {
        #[cfg(not(target_os = "macos"))]
        let _ = authority.native_roots;
        #[cfg(target_os = "macos")]
        if self.kind == DesktopHarnessKind::Claude
            && std::env::var("NANH_CLAUDE_MAC_NATIVE_CHAT").as_deref() == Ok("1")
        {
            let roots = authority.native_roots.ok_or(Reason::IsolationUnavailable)?;
            let mut ui = SemanticUi::Claude(Box::new(
                gui.claude_native_chat_session(&self.directory, roots)?,
            ));
            let outcome = complete_scenario(&mut ui, &scenario, &self.directory, result).await;
            return ui.finish(scenario.gate, outcome);
        }

        #[cfg(windows)]
        if self.kind == DesktopHarnessKind::Claude
            && std::env::var("NANH_CLAUDE_WINDOWS_NATIVE_CHAT").as_deref() == Ok("1")
        {
            let profile = authority
                .windows_profile
                .ok_or(Reason::IsolationUnavailable)?;
            let workspace = scenario
                .fixture
                .parent()
                .ok_or(Reason::IsolationUnavailable)?;
            let mut ui = SemanticUi::ClaudeWindows(gui.claude_windows_chat_session(
                &self.directory,
                profile,
                workspace,
            )?);
            let outcome = complete_scenario(&mut ui, &scenario, &self.directory, result).await;
            return ui.finish(scenario.gate, outcome);
        }

        #[cfg(target_os = "linux")]
        if self.kind == DesktopHarnessKind::Claude && crate::gui::claude_linux_chat_policy() {
            let mut ui =
                SemanticUi::ClaudeLinux(Box::new(gui.claude_linux_chat_session(&self.directory)?));
            let outcome = complete_scenario(&mut ui, &scenario, &self.directory, result).await;
            return ui.finish(scenario.gate, outcome);
        }

        let mut ui = match self.kind {
            DesktopHarnessKind::Zed => {
                SemanticUi::Zed(Box::new(gui.native_clipboard_session(&self.directory)?))
            }
            DesktopHarnessKind::Hermes => return Err(Reason::IsolationUnavailable),
            DesktopHarnessKind::Claude | DesktopHarnessKind::ChatGpt | DesktopHarnessKind::Pen => {
                return gui.inventory_renderer(
                    &self.directory,
                    owner.ok_or(Reason::ApplicationExited)?,
                    composer_observations,
                );
            }
        };
        let outcome = complete_scenario(&mut ui, &scenario, &self.directory, result).await;
        ui.finish(scenario.gate, outcome)
    }
}

enum SemanticUi<'a> {
    Zed(Box<NativeClipboardSession<'a>>),
    #[cfg(target_os = "macos")]
    Claude(Box<ClaudeNativeChatSession<'a>>),
    #[cfg(windows)]
    ClaudeWindows(ClaudeWindowsChatSession<'a>),
    #[cfg(target_os = "linux")]
    ClaudeLinux(Box<ClaudeLinuxChatSession<'a>>),
    Renderer(RendererSession<'a>),
    Codex(CodexDomSession<'a>),
}

impl SemanticUi<'_> {
    fn failure_prompt(&self) -> Result<String, Reason> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Claude(_) => semantic_marker("Check the expected provider failure"),
            _ => Ok("Check the expected provider failure".into()),
        }
    }
    fn methods(&self) -> (InputMode, ResponseVerification) {
        match self {
            Self::Zed(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeThreadExport,
            ),
            #[cfg(target_os = "macos")]
            Self::Claude(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeAssistantClipboard,
            ),
            #[cfg(windows)]
            Self::ClaudeWindows(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeAssistantClipboard,
            ),
            #[cfg(target_os = "linux")]
            Self::ClaudeLinux(_) => (
                InputMode::NativeClipboardAndKeyboard,
                ResponseVerification::NativeAssistantClipboard,
            ),
            Self::Renderer(_) | Self::Codex(_) => (
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
            #[cfg(target_os = "macos")]
            Self::Claude(session) => {
                session.prepare_turn(prompt, matches!(purpose, DomPurpose::Failure), gate)?;
                session.new_turn(prompt)?;
                match purpose {
                    DomPurpose::Response => {
                        session.wait_response(marker, Duration::from_secs(30), gate)
                    }
                    DomPurpose::Failure => session.wait_retry(Duration::from_secs(30), gate),
                }
            }
            #[cfg(windows)]
            Self::ClaudeWindows(session) => {
                session.new_turn(prompt)?;
                match purpose {
                    DomPurpose::Response => {
                        session.wait_response(marker, Duration::from_secs(30), gate)
                    }
                    DomPurpose::Failure => session.wait_retry(Duration::from_secs(30), gate),
                }
            }
            #[cfg(target_os = "linux")]
            Self::ClaudeLinux(session) => {
                session.new_turn(prompt)?;
                match purpose {
                    DomPurpose::Response => {
                        session.wait_response(marker, Duration::from_secs(30), gate)
                    }
                    DomPurpose::Failure => session.wait_retry(Duration::from_secs(30), gate),
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
            Self::Codex(session) => session.turn(
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
            #[cfg(target_os = "macos")]
            // Claude arms a request-specific failure epoch immediately before
            // its clean/Sent native turn, rather than failing detached requests.
            Self::Claude(_) => 503,
            #[cfg(windows)]
            Self::ClaudeWindows(_) => {
                gate.fail_recoverable_scenario(true);
                503
            }
            #[cfg(target_os = "linux")]
            Self::ClaudeLinux(_) => {
                gate.fail_recoverable_scenario(true);
                503
            }
            Self::Renderer(_) | Self::Codex(_) => {
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
            #[cfg(target_os = "macos")]
            Self::Claude(session) => {
                session.retry_once()?;
                session.wait_response(marker, Duration::from_secs(30), gate)
            }
            #[cfg(windows)]
            Self::ClaudeWindows(session) => {
                session.retry_once()?;
                session.wait_response(marker, Duration::from_secs(30), gate)
            }
            #[cfg(target_os = "linux")]
            Self::ClaudeLinux(session) => {
                session.retry_once()?;
                session.wait_response(marker, Duration::from_secs(30), gate)
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
            Self::Codex(session) => session.turn(
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
            #[cfg(target_os = "macos")]
            Self::Claude(session) => session.finish(gate, outcome),
            #[cfg(windows)]
            Self::ClaudeWindows(session) => session.finish(gate, outcome),
            #[cfg(target_os = "linux")]
            Self::ClaudeLinux(session) => session.finish(gate, outcome),
            Self::Renderer(_) | Self::Codex(_) => outcome,
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
    let owned_fixture_scope = {
        #[cfg(target_os = "macos")]
        {
            matches!(ui, SemanticUi::Claude(_)) && owned_read_fixture_policy(fixture)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    };
    let (selected, fixture_selection) =
        select_semantic_read_tool(&requests, fixture, owned_fixture_scope);
    record_inventory(
        directory,
        &requests,
        selected.is_some(),
        owned_fixture_scope,
        fixture_selection,
    )?;
    let (name, arguments) = selected.ok_or(Reason::ToolMismatch)?;
    let selected_tool = SelectedTool::from_name(&name).ok_or(Reason::ToolMismatch)?;
    let tool_marker = semantic_marker("NAN CHECK TOOL")?;
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
    record_provider_oracle(directory, "tool", &tool, gate, Some(selected_tool))?;
    verify_semantic_tool(&tool, gate, selected_tool, owned_fixture_scope)?;
    result.steps.push(CheckStep::ToolVerified);

    gate.arm_fixture_response("NAN_CHECK_EXPECTED_FAILURE")
        .map_err(|()| Reason::ProviderFailed)?;
    ui.inject_failure(gate, directory)?;
    let failure_prompt = ui.failure_prompt()?;
    let failure = ui.turn(
        &failure_prompt,
        "NAN_CHECK_EXPECTED_FAILURE",
        DomPurpose::Failure,
        gate,
    );
    record_provider_oracle(directory, "failure", &tool, gate, None)?;
    failure?;
    if !gate.failure_observed() {
        return Err(Reason::ProviderFailed);
    }
    let recovered_marker = semantic_marker("NAN CHECK RECOVERED")?;
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

fn verify_semantic_tool(
    tool: &ScriptedProvider,
    gate: &ProviderGate,
    selected_tool: SelectedTool,
    owned_fixture_scope: bool,
) -> Result<(), Reason> {
    #[cfg(not(target_os = "macos"))]
    let _ = (selected_tool, owned_fixture_scope);
    if !tool.completed()
        || !tool.recording_bounded()
        || !gate.tool_verified()
        || !gate.fixture_response_verified()
    {
        return Err(Reason::ToolMismatch);
    }
    #[cfg(target_os = "macos")]
    if owned_fixture_scope
        && matches!(selected_tool, SelectedTool::FixtureRead)
        && std::env::var("NANH_CLAUDE_MCP_FIXTURE").as_deref() == Ok("read-only")
    {
        gate.authorize_claude_fixture_failure(selected_tool, owned_fixture_scope)
            .map_err(|()| Reason::ProviderFailed)?;
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_result: Option<ToolResultObservation>,
}

fn record_provider_oracle(
    directory: &Path,
    stage: &'static str,
    tool: &ScriptedProvider,
    gate: &ProviderGate,
    selected_tool: Option<SelectedTool>,
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
        tool_result: selected_tool
            .map(|selected| ToolResultObservation::collect(&tool.chat_requests(), selected)),
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
#[serde(untagged)]
enum OwnedReadFixtureCount {
    Unavailable,
    Observed(usize),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    owned_read_fixture_tool_count: Option<OwnedReadFixtureCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owned_read_fixture_selection: Option<OwnedReadFixtureSelection>,
}

fn record_inventory(
    directory: &Path,
    requests: &[serde_json::Value],
    selected: bool,
    owned_fixture_scope: bool,
    fixture_selection: Option<OwnedReadFixtureSelection>,
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
        owned_read_fixture_selection: fixture_selection,
        owned_read_fixture_tool_count: owned_fixture_scope.then(|| {
            owned_read_fixture_count(requests).map_or(
                OwnedReadFixtureCount::Unavailable,
                OwnedReadFixtureCount::Observed,
            )
        }),
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

fn owned_read_fixture_count(requests: &[serde_json::Value]) -> Option<usize> {
    if requests.len() > 4096 {
        return None;
    }
    let mut inspected = 0_usize;
    let mut count = 0_usize;
    for request in requests {
        let Some(tools) = request.get("tools").and_then(serde_json::Value::as_array) else {
            continue;
        };
        inspected = inspected.checked_add(tools.len())?;
        if inspected > 4096 {
            return None;
        }
        count += tools
            .iter()
            .filter(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    == Some("mcp__nanh-read-fixture__read_file")
            })
            .count();
    }
    Some(count)
}

#[cfg(target_os = "macos")]
fn owned_read_fixture_policy(fixture: &Path) -> bool {
    use sha2::{Digest as _, Sha256};
    let expected = "ecb56f97d549f3040908f1bb8f0bb32235f9b48d9572ea348098135fe7999fc0";
    for (key, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "macOS"),
        ("NANH_CLAUDE_MAC_PROFILE_POLICY", "native-known-folders"),
        ("NANH_CLAUDE_MCP_FIXTURE", "read-only"),
        ("NANH_CLAUDE_MCP_SOURCE_SHA256", expected),
    ] {
        if std::env::var(key).as_deref() != Ok(value) {
            return false;
        }
    }
    let valid = || -> Option<()> {
        if !owned_read_fixture_workspace(fixture) {
            return None;
        }
        for key in ["NANH_CLAUDE_MCP_SCRIPT", "NANH_CLAUDE_MCP_PYTHON"] {
            let path = PathBuf::from(std::env::var_os(key)?);
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || !path.is_absolute() || path.canonicalize().ok()? != path {
                return None;
            }
            if key == "NANH_CLAUDE_MCP_SCRIPT" {
                if metadata.len() > 32768 {
                    return None;
                }
                let bytes = std::fs::read(path).ok()?;
                if Sha256::digest(bytes).as_slice()
                    != [
                        0xec, 0xb5, 0x6f, 0x97, 0xd5, 0x49, 0xf3, 0x04, 0x09, 0x08, 0xf1, 0xbb,
                        0x8f, 0x0b, 0xb3, 0x22, 0x35, 0xf9, 0xb4, 0x8d, 0x95, 0x72, 0xea, 0x34,
                        0x80, 0x98, 0x13, 0x5f, 0xe7, 0x99, 0x9f, 0xc0,
                    ]
                {
                    return None;
                }
            }
        }
        Some(())
    };
    valid().is_some()
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn owned_read_fixture_workspace(fixture: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let valid = || -> Option<()> {
        let workspace = fixture.parent()?;
        if !workspace.is_absolute()
            || fixture.file_name()? != "read-target.txt"
            || workspace.canonicalize().ok()?.as_path() != workspace
            || fixture.canonicalize().ok()?.as_path() != fixture
        {
            return None;
        }
        let directory = std::fs::symlink_metadata(workspace).ok()?;
        let entry = std::fs::symlink_metadata(fixture).ok()?;
        if !directory.is_dir()
            || directory.mode() & 0o077 != 0
            || !entry.is_file()
            || entry.nlink() != 1
            || entry.mode() & 0o077 != 0
            || directory.uid() != entry.uid()
            || entry.len() > 4096
        {
            return None;
        }
        let (file, _) = nan_harness_private_fs::open_private_read(fixture).ok()?;
        let opened = file.metadata().ok()?;
        (opened.dev() == entry.dev() && opened.ino() == entry.ino()).then_some(())
    };
    valid().is_some()
}

#[cfg(test)]
mod owned_fixture_tests {
    use super::owned_read_fixture_count;
    use serde_json::json;
    #[cfg(unix)]
    #[test]
    fn fixture_binds_private_parent_independently_of_worker_current_directory() {
        use super::owned_read_fixture_workspace;
        use std::io::Write as _;
        use std::os::unix::fs::{PermissionsExt as _, symlink};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap().join("workspace");
        nan_harness_private_fs::create_private_dir(&root).unwrap();
        let fixture = root.join("read-target.txt");
        nan_harness_private_fs::open_private_new(&fixture)
            .unwrap()
            .write_all(b"fixture")
            .unwrap();
        assert_ne!(std::env::current_dir().unwrap(), root);
        assert!(owned_read_fixture_workspace(&fixture));
        let wrong_name = root.join("different.txt");
        nan_harness_private_fs::open_private_new(&wrong_name).unwrap();
        assert!(!owned_read_fixture_workspace(&wrong_name));
        let alias = temporary.path().join("alias");
        symlink(&root, &alias).unwrap();
        assert!(!owned_read_fixture_workspace(
            &alias.join("read-target.txt")
        ));
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!owned_read_fixture_workspace(&fixture));
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.join("hard-link");
        std::fs::hard_link(&fixture, &link).unwrap();
        assert!(!owned_read_fixture_workspace(&fixture));
        std::fs::remove_file(link).unwrap();
        std::fs::write(&fixture, vec![b'x'; 4097]).unwrap();
        assert!(!owned_read_fixture_workspace(&fixture));
    }

    #[test]
    fn exact_fixture_offer_is_counted_without_selecting_aliases() {
        let request = json!({"tools":[
            {"function":{"name":"mcp__nanh-read-fixture__read_file"}},
            {"function":{"name":"mcp__nanh_read_fixture__read_file"}},
            {"function":{"name":"mcp__other__read_file"}},
            {"function":{"name":"Read"}},
            {"name":"mcp__nanh-read-fixture__read_file"}
        ]});
        assert_eq!(owned_read_fixture_count(&[request]), Some(1));
        assert_eq!(owned_read_fixture_count(&[json!({"tools":[]})]), Some(0));
        assert_eq!(
            owned_read_fixture_count(&[json!({"tools":vec![json!({});4097]})]),
            None
        );
        assert_eq!(owned_read_fixture_count(&vec![json!({}); 4097]), None);
    }
}
