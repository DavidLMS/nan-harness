use super::constants::ROUND_TRIP_MARKER;
use crate::scripted_provider::ScriptedToolCall;
use nan_harness_core::HarnessKind;
use std::ffi::OsString;
use std::path::Path;

pub(crate) enum RunKind {
    Inventory,
    Tool(ScriptedToolCall),
    Sentinel,
    External {
        tool: String,
        arguments: Vec<String>,
        enabled_tools: Vec<String>,
    },
}

pub(crate) fn headless_arguments(
    kind: HarnessKind,
    run_kind: &RunKind,
    marker: &str,
    workspace: &Path,
) -> Vec<OsString> {
    let prompt = headless_prompt(run_kind, marker);
    let mut arguments = headless_base_arguments(kind, run_kind, &prompt);
    if kind == HarnessKind::ClaudeCode {
        append_claude_arguments(&mut arguments, run_kind);
    }
    if kind == HarnessKind::QwenCode {
        append_qwen_arguments(&mut arguments, run_kind);
    }
    if kind == HarnessKind::PrimeAgent {
        append_prime_arguments(&mut arguments, workspace);
    }
    arguments
}

fn headless_prompt(run_kind: &RunKind, marker: &str) -> String {
    match run_kind {
        RunKind::Inventory | RunKind::Sentinel => {
            format!("Reply exactly {marker} without using tools.")
        }
        RunKind::Tool(tool) => format!(
            "Use the {} tool exactly once, wait for its result, then reply exactly {marker}.",
            tool.name
        ),
        RunKind::External { tool, .. } => format!(
            "Run the deterministic {tool} authorization scenario, report its controlled prerequisite, then reply exactly {marker}."
        ),
    }
}

fn headless_base_arguments(kind: HarnessKind, run_kind: &RunKind, prompt: &str) -> Vec<OsString> {
    match kind {
        HarnessKind::ClaudeCode => claude_base_arguments(prompt),
        HarnessKind::Codex => codex_base_arguments(prompt),
        HarnessKind::OpenCode => opencode_base_arguments(prompt),
        HarnessKind::Hermes => hermes_base_arguments(prompt),
        HarnessKind::Pi => pi_base_arguments(prompt),
        HarnessKind::PrimeAgent => prime_base_arguments(prompt),
        HarnessKind::Omp => omp_base_arguments(prompt),
        HarnessKind::DeepSeekHarness => deepseek_base_arguments(prompt),
        HarnessKind::OpenClaw => openclaw_base_arguments(prompt),
        HarnessKind::Cline => cline_base_arguments(prompt),
        HarnessKind::QwenCode => qwen_base_arguments(prompt),
        HarnessKind::KimiCode => kimi_base_arguments(prompt),
        HarnessKind::Aider => aider_arguments(run_kind, prompt),
        HarnessKind::Goose => goose_base_arguments(prompt),
        HarnessKind::Fx => fx_base_arguments(prompt),
    }
}

fn claude_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "-p".into(),
        prompt.to_owned().into(),
        "--permission-mode".into(),
        "bypassPermissions".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--no-session-persistence".into(),
        "--max-turns".into(),
        "12".into(),
    ]
}

fn codex_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "exec".into(),
        "--skip-git-repo-check".into(),
        "--ephemeral".into(),
        "--dangerously-bypass-approvals-and-sandbox".into(),
        "--json".into(),
        prompt.to_owned().into(),
    ]
}

fn opencode_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "run".into(),
        "--pure".into(),
        "--format".into(),
        "json".into(),
        "--auto".into(),
        prompt.to_owned().into(),
    ]
}

fn hermes_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "chat".into(),
        "--query".into(),
        prompt.to_owned().into(),
        "--quiet".into(),
        "--yolo".into(),
        "--safe-mode".into(),
        "--source".into(),
        "tool".into(),
        "--max-turns".into(),
        "12".into(),
    ]
}

fn pi_base_arguments(prompt: &str) -> Vec<OsString> {
    pi_family_base_arguments("read,bash,edit,write,grep,find,ls", prompt)
}

fn prime_base_arguments(prompt: &str) -> Vec<OsString> {
    pi_family_base_arguments("ipython", prompt)
}

fn pi_family_base_arguments(tools: &str, prompt: &str) -> Vec<OsString> {
    vec![
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
        tools.into(),
        prompt.to_owned().into(),
    ]
}

fn omp_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "--mode".into(),
        "json".into(),
        "--print".into(),
        "--no-session".into(),
        "--no-extensions".into(),
        "--no-skills".into(),
        "--no-rules".into(),
        "--no-lsp".into(),
        "--no-title".into(),
        "--tools".into(),
        "read,bash,edit,write,grep,glob".into(),
        prompt.to_owned().into(),
    ]
}

fn deepseek_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "--profile".into(),
        "headless".into(),
        prompt.to_owned().into(),
    ]
}

fn openclaw_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "agent".into(),
        "--local".into(),
        "--session-id".into(),
        "nan-harness-conformance".into(),
        "--message".into(),
        prompt.to_owned().into(),
        "--json".into(),
    ]
}

fn cline_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "--json".into(),
        "--timeout".into(),
        "60".into(),
        prompt.to_owned().into(),
    ]
}

fn qwen_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "--safe-mode".into(),
        "--prompt".into(),
        prompt.to_owned().into(),
        "--output-format".into(),
        "json".into(),
    ]
}

fn kimi_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "--prompt".into(),
        prompt.to_owned().into(),
        "--output-format".into(),
        "stream-json".into(),
    ]
}

fn goose_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "run".into(),
        "--no-profile".into(),
        "--no-session".into(),
        "--with-builtin".into(),
        "developer".into(),
        "--output-format".into(),
        "json".into(),
        "--text".into(),
        prompt.to_owned().into(),
    ]
}

fn fx_base_arguments(prompt: &str) -> Vec<OsString> {
    vec![
        "ask".into(),
        "--yolo".into(),
        "--no-save".into(),
        "--no-color".into(),
        prompt.to_owned().into(),
    ]
}

fn aider_arguments(run_kind: &RunKind, prompt: &str) -> Vec<OsString> {
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
            prompt.to_owned().into(),
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

fn append_claude_arguments(arguments: &mut Vec<OsString>, run_kind: &RunKind) {
    let Some((enabled_tools, scenario_arguments)) = claude_arguments(run_kind) else {
        return;
    };
    let enabled_tools = enabled_tools.join(",");
    arguments.extend([
        OsString::from("--tools"),
        OsString::from(enabled_tools.clone()),
        OsString::from("--allowedTools"),
        OsString::from(enabled_tools),
    ]);
    arguments.extend(scenario_arguments.into_iter().map(OsString::from));
}

fn claude_arguments(run_kind: &RunKind) -> Option<(Vec<String>, Vec<String>)> {
    match run_kind {
        RunKind::External {
            enabled_tools,
            arguments,
            ..
        } => Some((enabled_tools.clone(), arguments.clone())),
        RunKind::Tool(tool) => Some((vec![tool.name.clone()], Vec::new())),
        RunKind::Inventory | RunKind::Sentinel => None,
    }
}

fn append_qwen_arguments(arguments: &mut Vec<OsString>, run_kind: &RunKind) {
    if matches!(run_kind, RunKind::Tool(_)) {
        arguments.extend([
            OsString::from("--allowed-tools"),
            OsString::from("read_file"),
        ]);
    }
}

fn append_prime_arguments(arguments: &mut Vec<OsString>, workspace: &Path) {
    let socket = workspace.join("home/prime-agent.sock");
    arguments.extend([OsString::from("--daemon-socket"), socket.into_os_string()]);
}
