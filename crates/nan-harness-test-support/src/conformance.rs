use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::{TerminalCommand, TerminalOutput};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const CONFORMANCE_SCHEMA_VERSION: u8 = 1;
pub const TEST_CREDENTIAL: &str = "nan-harness-conformance-test-credential";
pub const INVENTORY_MARKER: &str = "NAN_HARNESS_CONFORMANCE_INVENTORY_OK";
pub const SENTINEL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_SENTINEL_OK";
pub const ROUND_TRIP_MARKER: &str = "NAN_HARNESS_CONFORMANCE_ROUND_TRIP_OK";
pub const EXTERNAL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_EXTERNAL_OK";
const EXTERNAL_DIAGNOSTIC: &str = "DesignSync needs design-system authorization";

const MAX_DURATION_MILLISECONDS: u64 = 86_400_000;

const CLAUDE_INVENTORY: &[&str] = &[
    "Agent",
    "Bash",
    "CronCreate",
    "CronDelete",
    "CronList",
    "DesignSync",
    "Edit",
    "EnterWorktree",
    "ExitWorktree",
    "NotebookEdit",
    "Read",
    "ReportFindings",
    "ScheduleWakeup",
    "SendMessage",
    "Skill",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TaskUpdate",
    "WebFetch",
    "WebSearch",
    "Workflow",
    "Write",
];
const CODEX_INVENTORY: &[&str] = &["apply_patch", "exec_command", "update_plan", "write_stdin"];
const OPENCODE_INVENTORY: &[&str] = &[
    "bash",
    "edit",
    "glob",
    "grep",
    "read",
    "skill",
    "task",
    "todowrite",
    "webfetch",
    "write",
];
const HERMES_INVENTORY: &[&str] = &[
    "clarify",
    "cronjob",
    "delegate_task",
    "execute_code",
    "memory",
    "patch",
    "process",
    "read_file",
    "search_files",
    "session_search",
    "skill_manage",
    "skill_view",
    "skills_list",
    "terminal",
    "text_to_speech",
    "todo",
    "write_file",
];
const PI_INVENTORY: &[&str] = &["bash", "edit", "find", "grep", "ls", "read", "write"];
const PRIME_AGENT_INVENTORY: &[&str] = &["ipython"];
const DEEPSEEK_HARNESS_INVENTORY: &[&str] = &[
    "bash",
    "create_goal",
    "edit",
    "exit_plan_mode",
    "get_goal",
    "glob",
    "grep",
    "interrupt_agent",
    "job_kill",
    "job_list",
    "job_output",
    "list_agents",
    "ralph",
    "read",
    "read_image",
    "send_message",
    "skill",
    "str_replace_editor",
    "subagent",
    "subagent_fork",
    "todo_write",
    "update_goal",
    "workflow",
    "write",
];
const OPENCLAW_INVENTORY: &[&str] = &[
    "agents_list",
    "apply_patch",
    "browser",
    "canvas",
    "create_goal",
    "cron",
    "dir_fetch",
    "dir_list",
    "edit",
    "exec",
    "file_fetch",
    "file_write",
    "gateway",
    "get_goal",
    "image",
    "memory_get",
    "memory_search",
    "message",
    "node_inference",
    "nodes",
    "process",
    "read",
    "session_status",
    "sessions_history",
    "sessions_list",
    "sessions_send",
    "sessions_spawn",
    "sessions_yield",
    "skill_workshop",
    "subagents",
    "tts",
    "update_goal",
    "web_fetch",
    "web_search",
    "write",
];
const CLINE_INVENTORY: &[&str] = &[
    "ask_question",
    "editor",
    "fetch_web_content",
    "read_files",
    "run_commands",
    "search_codebase",
    "spawn_agent",
    "team_attach_outcome_fragment",
    "team_await_runs",
    "team_broadcast",
    "team_cancel_run",
    "team_cleanup",
    "team_create_outcome",
    "team_finalize_outcome",
    "team_list_outcomes",
    "team_list_runs",
    "team_mission_log",
    "team_read_mailbox",
    "team_review_outcome_fragment",
    "team_run_task",
    "team_send_message",
    "team_shutdown_teammate",
    "team_spawn_teammate",
    "team_status",
    "team_task",
];
const QWEN_CODE_INVENTORY: &[&str] = &[
    "agent",
    "get_goal",
    "glob",
    "grep_search",
    "list_agents",
    "list_directory",
    "read_file",
    "skill",
    "todo_write",
    "tool_search",
    "update_goal",
];
const KIMI_CODE_INVENTORY: &[&str] = &[
    "Agent",
    "AgentSwarm",
    "AskUserQuestion",
    "Bash",
    "CreateGoal",
    "CronCreate",
    "CronDelete",
    "CronList",
    "Edit",
    "EnterPlanMode",
    "ExitPlanMode",
    "FetchURL",
    "GetGoal",
    "Glob",
    "Grep",
    "Read",
    "ReadMediaFile",
    "SetGoalBudget",
    "Skill",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TodoList",
    "UpdateGoal",
    "WaitFor",
    "Write",
];
const GOOSE_INVENTORY: &[&str] = &["edit", "read_image", "shell", "tree", "write"];
const FX_INVENTORY: &[&str] = &[
    "ask_user_question",
    "copy_file",
    "create_folder",
    "delete_file",
    "edit_file",
    "file_info",
    "glob_files",
    "grep_files",
    "install_skill",
    "list_files",
    "mcp_features",
    "mcp_search_tools",
    "mcp_select_tool",
    "memory",
    "open_file",
    "perplexity_search",
    "read_file",
    "read_tool_result",
    "rename_file",
    "semantic_search",
    "skill",
    "subagent",
    "terminal",
    "vision",
    "web_fetch",
    "write_file",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessRegistration {
    pub kind: HarnessKind,
    pub binary_name: &'static str,
    pub inventory: &'static [&'static str],
}

const REGISTRY: [HarnessRegistration; 14] = [
    registration(HarnessKind::ClaudeCode, "claude", CLAUDE_INVENTORY),
    registration(HarnessKind::Codex, "codex", CODEX_INVENTORY),
    registration(HarnessKind::OpenCode, "opencode", OPENCODE_INVENTORY),
    registration(HarnessKind::Hermes, "hermes", HERMES_INVENTORY),
    registration(HarnessKind::Pi, "pi", PI_INVENTORY),
    registration(
        HarnessKind::PrimeAgent,
        "prime-agent",
        PRIME_AGENT_INVENTORY,
    ),
    registration(
        HarnessKind::DeepSeekHarness,
        "dsh",
        DEEPSEEK_HARNESS_INVENTORY,
    ),
    registration(HarnessKind::OpenClaw, "openclaw", OPENCLAW_INVENTORY),
    registration(HarnessKind::Cline, "cline", CLINE_INVENTORY),
    registration(HarnessKind::QwenCode, "qwen", QWEN_CODE_INVENTORY),
    registration(HarnessKind::KimiCode, "kimi", KIMI_CODE_INVENTORY),
    registration(HarnessKind::Aider, "aider", &["edit-protocol"]),
    registration(HarnessKind::Goose, "goose", GOOSE_INVENTORY),
    registration(HarnessKind::Fx, "fx", FX_INVENTORY),
];

const fn registration(
    kind: HarnessKind,
    binary_name: &'static str,
    inventory: &'static [&'static str],
) -> HarnessRegistration {
    HarnessRegistration {
        kind,
        binary_name,
        inventory,
    }
}

#[must_use]
pub fn harness_registry() -> &'static [HarnessRegistration] {
    &REGISTRY
}

#[must_use]
pub fn harness_registration(kind: HarnessKind) -> Option<&'static HarnessRegistration> {
    REGISTRY
        .iter()
        .find(|registration| registration.kind == kind)
}

/// Validates that the registry has one non-empty entry for every harness kind.
///
/// # Errors
///
/// Returns [`RegistryError`] when the registry count, identities, or inventories are invalid.
pub fn validate_harness_registry() -> Result<(), RegistryError> {
    if REGISTRY.len() != HarnessKind::ALL.len() {
        return Err(RegistryError::Count {
            expected: HarnessKind::ALL.len(),
            actual: REGISTRY.len(),
        });
    }
    let mut kinds = BTreeSet::new();
    for registration in REGISTRY {
        if !kinds.insert(registration.kind) {
            return Err(RegistryError::Duplicate(registration.kind));
        }
        if registration.inventory.is_empty() {
            return Err(RegistryError::EmptyInventory(registration.kind));
        }
    }
    for kind in HarnessKind::ALL {
        if !kinds.contains(&kind) {
            return Err(RegistryError::Missing(kind));
        }
    }
    Ok(())
}

/// Builds a clean command prefix for a conformance test process.
#[must_use]
pub fn conformance_command(
    nan_harness: impl Into<PathBuf>,
    harness: HarnessKind,
    workspace: impl AsRef<Path>,
    provider_base_url: &str,
) -> TerminalCommand {
    TerminalCommand::new(nan_harness, workspace.as_ref())
        .clear_environment()
        .args([
            OsString::from(harness.binary_name()),
            OsString::from("--provider-base-url"),
            OsString::from(provider_base_url),
            OsString::from("--"),
        ])
        .env("CI", "1")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("NAN_API_KEY", TEST_CREDENTIAL)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env(
            "NAN_HARNESS_CONFIG_DIR",
            workspace.as_ref().join("nan-config"),
        )
        .env("HOME", workspace.as_ref())
        .timeout(Duration::from_secs(90))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("harness registry contains {actual} entries; expected {expected}")]
    Count { expected: usize, actual: usize },
    #[error("harness registry contains duplicate {0}")]
    Duplicate(HarnessKind),
    #[error("harness registry is missing {0}")]
    Missing(HarnessKind),
    #[error("harness registry has no inventory for {0}")]
    EmptyInventory(HarnessKind),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceCheck {
    pub name: String,
    pub status: ConformanceStatus,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceScenario {
    pub name: String,
    pub status: ConformanceStatus,
    pub checks: Vec<ConformanceCheck>,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceReport {
    pub schema_version: u8,
    pub harness: HarnessKind,
    pub scenarios: Vec<ConformanceScenario>,
    pub outcome: ConformanceOutcome,
    pub duration_milliseconds: u64,
}

impl ConformanceReport {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.outcome == ConformanceOutcome::Passed
    }
}

#[derive(Debug)]
pub struct PublishedConformanceRunner {
    nan_harness: PathBuf,
    harness: HarnessKind,
}

impl PublishedConformanceRunner {
    #[must_use]
    pub fn new(nan_harness: impl Into<PathBuf>, harness: HarnessKind) -> Self {
        let nan_harness = nan_harness.into();
        let nan_harness = if nan_harness.is_absolute() {
            nan_harness
        } else {
            std::env::current_dir()
                .map_or(nan_harness.clone(), |directory| directory.join(nan_harness))
        };
        Self {
            nan_harness,
            harness,
        }
    }

    /// Runs the deterministic published-release contracts.
    ///
    /// # Errors
    ///
    /// Returns [`ConformanceError`] when the registry cannot be validated or the runner cannot
    /// start a test process.
    pub async fn run(self) -> Result<ConformanceReport, ConformanceError> {
        validate_harness_registry().map_err(ConformanceError::Registry)?;
        let registration = harness_registration(self.harness).ok_or(ConformanceError::Registry(
            RegistryError::Missing(self.harness),
        ))?;
        let started = Instant::now();
        let mut scenarios = Vec::with_capacity(4);
        scenarios.push(self.run_inventory(registration).await);
        scenarios.push(self.run_tool_round_trip(registration).await);
        scenarios.push(self.run_sentinel(registration).await);
        scenarios.push(self.run_external_prerequisite(registration).await);
        let outcome = scenarios.iter().all(|scenario| {
            matches!(
                scenario.status,
                ConformanceStatus::Passed | ConformanceStatus::Skipped
            )
        });
        Ok(ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: self.harness,
            scenarios,
            outcome: if outcome {
                ConformanceOutcome::Passed
            } else {
                ConformanceOutcome::Failed
            },
            duration_milliseconds: duration_milliseconds(started.elapsed()),
        })
    }

    async fn run_inventory(&self, registration: &HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("inventory", started);
        };
        let Ok(provider) =
            ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
        else {
            return failed_scenario("inventory", started);
        };
        let output = self
            .run_process(
                registration,
                &workspace,
                &provider,
                RunKind::Inventory,
                INVENTORY_MARKER,
            )
            .await;
        let requests = provider.chat_requests();
        let _ = provider.shutdown().await;
        let status = match output {
            Ok(output)
                if output.status.success()
                    && inventory_matches(registration.kind, &tool_names(&requests)) =>
            {
                ConformanceStatus::Passed
            }
            _ => ConformanceStatus::Failed,
        };
        scenario("inventory", status, started)
    }

    async fn run_tool_round_trip(&self, registration: &HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("tool-round-trip", started);
        };
        let tool = round_trip_tool(registration.kind, workspace.path());
        let provider_scenario = if registration.kind == HarnessKind::Aider {
            ProviderScenario::inventory(format!(
                "edit-target.txt\n```text\n{ROUND_TRIP_MARKER}\n```\n"
            ))
        } else {
            ProviderScenario::tool(tool.name.clone(), tool.input.clone(), ROUND_TRIP_MARKER)
        };
        let Ok(provider) = ScriptedProvider::start(provider_scenario).await else {
            return failed_scenario("tool-round-trip", started);
        };
        let output = self
            .run_process(
                registration,
                &workspace,
                &provider,
                RunKind::Tool(tool.clone()),
                ROUND_TRIP_MARKER,
            )
            .await;
        let requests = provider.chat_requests();
        let _ = provider.shutdown().await;
        let status = match output {
            Ok(output)
                if output.status.success()
                    && output.stdout.contains(ROUND_TRIP_MARKER)
                    && ((registration.kind == HarnessKind::Aider
                        && !requests.is_empty()
                        && workspace
                            .read("edit-target.txt")
                            .is_ok_and(|contents| contents.contains(ROUND_TRIP_MARKER)))
                        || (registration.kind != HarnessKind::Aider
                            && tool_result_present(&requests)
                            && !tool_result_failed(&requests))) =>
            {
                ConformanceStatus::Passed
            }
            _ => ConformanceStatus::Failed,
        };
        scenario("tool-round-trip", status, started)
    }

    async fn run_sentinel(&self, registration: &HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("sentinel", started);
        };
        let Ok(provider) =
            ScriptedProvider::start(ProviderScenario::inventory(SENTINEL_MARKER)).await
        else {
            return failed_scenario("sentinel", started);
        };
        let output = self
            .run_process(
                registration,
                &workspace,
                &provider,
                RunKind::Sentinel,
                SENTINEL_MARKER,
            )
            .await;
        let _ = provider.shutdown().await;
        let status = match output {
            Ok(output) if output.status.success() && output.stdout.contains(SENTINEL_MARKER) => {
                ConformanceStatus::Passed
            }
            _ => ConformanceStatus::Failed,
        };
        scenario("sentinel", status, started)
    }

    async fn run_external_prerequisite(
        &self,
        registration: &HarnessRegistration,
    ) -> ConformanceScenario {
        let started = Instant::now();
        if registration.kind != HarnessKind::ClaudeCode {
            return scenario("external-prerequisite", ConformanceStatus::Skipped, started);
        }
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("external-prerequisite", started);
        };
        let tool = ScriptedToolCall {
            name: "DesignSync".to_owned(),
            input: json!({"method": "list_projects"}),
            result_expected: true,
        };
        let Ok(provider) = ScriptedProvider::start(ProviderScenario::tool(
            tool.name.clone(),
            tool.input,
            EXTERNAL_MARKER,
        ))
        .await
        else {
            return failed_scenario("external-prerequisite", started);
        };
        let output = self
            .run_process(
                registration,
                &workspace,
                &provider,
                RunKind::External,
                EXTERNAL_MARKER,
            )
            .await;
        let _ = provider.shutdown().await;
        let status = match output {
            Ok(output)
                if output.status.success()
                    && output.stdout.contains(EXTERNAL_MARKER)
                    && output.stdout.contains(EXTERNAL_DIAGNOSTIC) =>
            {
                ConformanceStatus::Passed
            }
            _ => ConformanceStatus::Failed,
        };
        scenario("external-prerequisite", status, started)
    }

    async fn run_process(
        &self,
        registration: &HarnessRegistration,
        workspace: &ConformanceWorkspace,
        provider: &ScriptedProvider,
        kind: RunKind,
        marker: &str,
    ) -> Result<TerminalOutput, ConformanceError> {
        let mut arguments = vec![
            OsString::from(registration.binary_name),
            OsString::from("--provider-base-url"),
            OsString::from(provider.base_url()),
            OsString::from("--"),
        ];
        arguments.extend(headless_arguments(
            registration.kind,
            &kind,
            marker,
            workspace.path(),
        ));
        let mut command = TerminalCommand::new(&self.nan_harness, workspace.path())
            .clear_environment()
            .args(arguments)
            .env("CI", "1")
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("NAN_API_KEY", TEST_CREDENTIAL)
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("NAN_NO_UPDATE_CHECK", "1")
            .env(
                "NAN_HARNESS_CONFIG_DIR",
                workspace.path().join("nan-config"),
            )
            .env("HOME", workspace.path().join("home"))
            .timeout(Duration::from_secs(90));
        if registration.kind == HarnessKind::ClaudeCode {
            command = command
                .env("CLAUDE_CONFIG_DIR", workspace.claude_config_path())
                .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
        }
        if registration.kind == HarnessKind::OpenCode {
            let home = workspace.path().join("home");
            command = command
                .env("XDG_CONFIG_HOME", home.join("config"))
                .env("XDG_DATA_HOME", home.join("data"))
                .env("XDG_CACHE_HOME", home.join("cache"));
        }
        if matches!(registration.kind, HarnessKind::Pi | HarnessKind::PrimeAgent) {
            command = command.env(
                "PI_CODING_AGENT_DIR",
                workspace.path().join("home/pi-agent"),
            );
        }
        if registration.kind == HarnessKind::DeepSeekHarness {
            command = command.env("DSH_HOME", workspace.path().join("home/dsh"));
        }
        if registration.kind == HarnessKind::PrimeAgent {
            command = command.env("PI_OFFLINE", "1");
        }
        command.run().await.map_err(ConformanceError::Terminal)
    }
}

#[derive(Debug, Clone)]
enum RunKind {
    Inventory,
    Tool(ScriptedToolCall),
    Sentinel,
    External,
}

#[allow(clippy::too_many_lines)]
fn headless_arguments(
    kind: HarnessKind,
    run_kind: &RunKind,
    marker: &str,
    workspace: &Path,
) -> Vec<OsString> {
    let is_inventory = matches!(run_kind, RunKind::Inventory | RunKind::Sentinel);
    let prompt = match run_kind {
        RunKind::Inventory | RunKind::Sentinel => {
            format!("Reply exactly {marker} without using tools.")
        }
        RunKind::Tool(tool) => format!(
            "Use the {} tool exactly once, wait for its result, then reply exactly {marker}.",
            tool.name
        ),
        RunKind::External => {
            format!(
                "Use DesignSync once, report its controlled authorization prerequisite, then reply exactly {marker}."
            )
        }
    };
    let mut arguments = match kind {
        HarnessKind::ClaudeCode => vec![
            "-p".into(),
            prompt.into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--no-session-persistence".into(),
            "--max-turns".into(),
            "12".into(),
        ],
        HarnessKind::Codex => vec![
            "exec".into(),
            "--skip-git-repo-check".into(),
            "--ephemeral".into(),
            "--json".into(),
            prompt.into(),
        ],
        HarnessKind::OpenCode => vec![
            "run".into(),
            "--pure".into(),
            "--format".into(),
            "json".into(),
            "--auto".into(),
            prompt.into(),
        ],
        HarnessKind::Hermes => vec![
            "chat".into(),
            "--query".into(),
            prompt.into(),
            "--quiet".into(),
            "--yolo".into(),
            "--safe-mode".into(),
            "--max-turns".into(),
            "12".into(),
        ],
        HarnessKind::Pi | HarnessKind::PrimeAgent => vec![
            "--mode".into(),
            "json".into(),
            "--print".into(),
            "--no-session".into(),
            "--no-extensions".into(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--no-themes".into(),
            "--no-context-files".into(),
            "--tools".into(),
            if kind == HarnessKind::PrimeAgent {
                "ipython".into()
            } else {
                "read,bash,edit,write,grep,find,ls".into()
            },
            prompt.into(),
        ],
        HarnessKind::DeepSeekHarness => vec!["--profile".into(), "headless".into(), prompt.into()],
        HarnessKind::OpenClaw => vec![
            "agent".into(),
            "--local".into(),
            "--session-id".into(),
            "nan-harness-conformance".into(),
            "--message".into(),
            prompt.into(),
            "--json".into(),
        ],
        HarnessKind::Cline => vec![
            "--json".into(),
            "--timeout".into(),
            "90".into(),
            prompt.into(),
        ],
        HarnessKind::QwenCode => vec![
            "--safe-mode".into(),
            "--prompt".into(),
            prompt.into(),
            "--output-format".into(),
            "json".into(),
        ],
        HarnessKind::KimiCode => vec![
            "--prompt".into(),
            prompt.into(),
            "--output-format".into(),
            "stream-json".into(),
        ],
        HarnessKind::Aider => {
            if matches!(run_kind, RunKind::Tool(_)) {
                vec![
                    "--message".into(),
                    format!("Replace the entire file with {ROUND_TRIP_MARKER}.").into(),
                    "--yes-always".into(),
                    "--no-auto-commits".into(),
                    "--no-git".into(),
                    "--edit-format".into(),
                    "whole".into(),
                    "--no-show-model-warnings".into(),
                    "--no-check-update".into(),
                    "--map-tokens".into(),
                    "0".into(),
                    "edit-target.txt".into(),
                ]
            } else {
                vec![
                    "--message".into(),
                    prompt.clone().into(),
                    "--yes-always".into(),
                    "--no-auto-commits".into(),
                    "--no-git".into(),
                    "--no-show-model-warnings".into(),
                    "--no-check-update".into(),
                    "--map-tokens".into(),
                    "0".into(),
                ]
            }
        }
        HarnessKind::Goose => vec![
            "run".into(),
            "--no-profile".into(),
            "--no-session".into(),
            "--with-builtin".into(),
            "developer".into(),
            "--output-format".into(),
            "json".into(),
            "--text".into(),
            prompt.into(),
        ],
        HarnessKind::Fx => vec![
            "ask".into(),
            "--yolo".into(),
            "--no-save".into(),
            "--no-color".into(),
            prompt.into(),
        ],
    };
    if kind == HarnessKind::ClaudeCode && !is_inventory {
        let tool = match run_kind {
            RunKind::External => "DesignSync",
            RunKind::Tool(_) => "Read",
            RunKind::Inventory | RunKind::Sentinel => "",
        };
        arguments.extend([
            OsString::from("--tools"),
            OsString::from(tool),
            OsString::from("--allowedTools"),
            OsString::from(tool),
        ]);
    }
    if kind == HarnessKind::QwenCode && matches!(run_kind, RunKind::Tool(_)) {
        arguments.extend([
            OsString::from("--allowed-tools"),
            OsString::from("read_file"),
        ]);
    }
    if kind == HarnessKind::PrimeAgent {
        arguments.extend([
            OsString::from("--daemon-socket"),
            workspace.join("home/prime-agent.sock").into_os_string(),
        ]);
    }
    arguments
}

fn round_trip_tool(kind: HarnessKind, workspace: &Path) -> ScriptedToolCall {
    let path = workspace.join("read-target.txt");
    let path = path.to_string_lossy().into_owned();
    let (name, input) = match kind {
        HarnessKind::ClaudeCode => ("Read", json!({"file_path": path})),
        HarnessKind::Codex => ("exec_command", json!({"cmd": "printf NAN_HARNESS_TOOL_OK"})),
        HarnessKind::OpenCode => ("bash", json!({"command": "printf NAN_HARNESS_TOOL_OK"})),
        HarnessKind::Hermes => ("read_file", json!({"path": path})),
        HarnessKind::Pi | HarnessKind::OpenClaw => ("read", json!({"path": path})),
        HarnessKind::PrimeAgent => ("ipython", json!({"code": "print('NAN_HARNESS_TOOL_OK')"})),
        HarnessKind::DeepSeekHarness => ("read", json!({"file_path": path})),
        HarnessKind::Cline => ("read_files", json!({"files": [{"path": path}]})),
        HarnessKind::QwenCode => ("read_file", json!({"file_path": path})),
        HarnessKind::KimiCode => ("Read", json!({"path": "read-target.txt"})),
        HarnessKind::Aider => ("edit-protocol", json!({})),
        HarnessKind::Goose => (
            "write",
            json!({"path": "round-trip.txt", "content": "NAN_HARNESS_TOOL_OK\n"}),
        ),
        HarnessKind::Fx => ("read_file", json!({"path": "read-target.txt"})),
    };
    ScriptedToolCall {
        name: name.to_owned(),
        input,
        result_expected: true,
    }
}

fn inventory_matches(kind: HarnessKind, actual: &BTreeSet<String>) -> bool {
    if kind == HarnessKind::Aider {
        return actual.is_empty();
    }
    let Some(registration) = harness_registration(kind) else {
        return false;
    };
    let expected = registration
        .inventory
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    if kind == HarnessKind::Hermes {
        let mut browser = actual
            .difference(&expected)
            .cloned()
            .collect::<BTreeSet<_>>();
        browser.remove("computer_use");
        return expected.is_subset(actual)
            && (browser == BTreeSet::from(["browser_exec".to_owned()])
                || browser
                    == BTreeSet::from([
                        "browser_back".to_owned(),
                        "browser_click".to_owned(),
                        "browser_console".to_owned(),
                        "browser_get_images".to_owned(),
                        "browser_navigate".to_owned(),
                        "browser_press".to_owned(),
                        "browser_scroll".to_owned(),
                        "browser_snapshot".to_owned(),
                        "browser_type".to_owned(),
                    ]));
    }
    actual == &expected
}

fn tool_names(requests: &[Value]) -> BTreeSet<String> {
    requests
        .iter()
        .flat_map(|request| {
            request
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|tool| {
            tool.pointer("/function/name")
                .or_else(|| tool.get("name"))
                .and_then(Value::as_str)
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn tool_result_present(requests: &[Value]) -> bool {
    requests.iter().any(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages.iter().any(|message| {
                    message.get("role").and_then(Value::as_str) == Some("tool")
                        && message.get("tool_call_id").is_some()
                })
            })
    })
}

fn tool_result_failed(requests: &[Value]) -> bool {
    requests.iter().any(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
            .filter_map(|message| message.get("content"))
            .any(value_is_error)
    })
}

fn value_is_error(value: &Value) -> bool {
    let text = value
        .as_str()
        .map(str::trim_start)
        .unwrap_or_default()
        .to_ascii_lowercase();
    text.starts_with("error")
        || text.starts_with("<system>error:")
        || value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

fn scenario(name: &str, status: ConformanceStatus, started: Instant) -> ConformanceScenario {
    let duration = duration_milliseconds(started.elapsed());
    ConformanceScenario {
        name: name.to_owned(),
        status,
        checks: vec![ConformanceCheck {
            name: "contract".to_owned(),
            status,
            duration_milliseconds: duration,
        }],
        duration_milliseconds: duration,
    }
}

fn failed_scenario(name: &str, started: Instant) -> ConformanceScenario {
    scenario(name, ConformanceStatus::Failed, started)
}

fn duration_milliseconds(duration: Duration) -> u64 {
    duration
        .as_millis()
        .try_into()
        .unwrap_or(MAX_DURATION_MILLISECONDS)
        .min(MAX_DURATION_MILLISECONDS)
}

#[derive(Debug, Error)]
pub enum ConformanceError {
    #[error(transparent)]
    Registry(RegistryError),
    #[error(transparent)]
    Terminal(#[from] crate::terminal::TerminalError),
}

#[cfg(test)]
mod tests {
    use super::{
        CONFORMANCE_SCHEMA_VERSION, ConformanceOutcome, ConformanceReport, ConformanceStatus,
        harness_registry, validate_harness_registry,
    };
    use nan_harness_core::HarnessKind;

    #[test]
    fn registry_covers_every_harness_kind() {
        validate_harness_registry().expect("the conformance registry should be complete");
        let kinds = harness_registry()
            .iter()
            .map(|registration| registration.kind)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(kinds.len(), HarnessKind::ALL.len());
        assert!(HarnessKind::ALL.iter().all(|kind| kinds.contains(kind)));
    }

    #[test]
    fn report_serialization_is_bounded_and_safe() {
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: HarnessKind::ClaudeCode,
            scenarios: vec![],
            outcome: ConformanceOutcome::Passed,
            duration_milliseconds: 3,
        };
        let encoded = serde_json::to_string(&report).expect("report should serialize");
        assert!(encoded.contains("schemaVersion"));
        assert!(encoded.contains("durationMilliseconds"));
        assert!(!encoded.contains("prompt"));
        assert!(!encoded.contains("credential"));
        assert!(matches!(
            report.outcome,
            ConformanceOutcome::Passed | ConformanceOutcome::Failed
        ));
        assert!(matches!(
            ConformanceStatus::Skipped,
            ConformanceStatus::Skipped
        ));
    }
}
