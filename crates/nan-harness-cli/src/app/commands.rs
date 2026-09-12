mod auth;
mod completions;
mod diagnostics;
mod telemetry;

use super::args::{
    BridgedHarnessRunArgs, ChatGptDesktopArgs, ClaudeDesktopArgs, ConfigArgs, DirectHarnessRunArgs,
    DoctorArgs, HermesDesktopArgs, PenDesktopArgs, RecordInstallationArgs, SearchCommand,
    UninstallArgs, ZedDesktopArgs,
};
pub(crate) use auth::AuthCommand;
use clap::Subcommand;
pub(crate) use completions::CompletionShell;
pub(crate) use diagnostics::LocalDiagnosticsCommand;
pub(crate) use telemetry::TelemetryCommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(about = nan_harness_i18n::messages::help_show_or_change_the_terminal_language(nan_harness_i18n::locale()))]
    Language {
        #[arg(value_name = "LANGUAGE")]
        language: Option<String>,
    },
    #[command(
        name = "chatgpt-desktop",
        visible_alias = "codex-desktop",
        about = nan_harness_i18n::messages::help_run_chatgpt_desktop_through_nan_experimental(nan_harness_i18n::locale())
    )]
    ChatGptDesktop(ChatGptDesktopArgs),
    #[command(
        name = "claude-desktop",
        about = nan_harness_i18n::messages::help_run_claude_desktop_through_nan_experimental(nan_harness_i18n::locale())
    )]
    ClaudeDesktop(ClaudeDesktopArgs),
    #[command(
        name = "claude",
        visible_alias = "claude-code",
        about = nan_harness_i18n::messages::help_run_claude_code_through_the_local_nan_harness_bridge(nan_harness_i18n::locale())
    )]
    Claude(BridgedHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_codex_through_the_local_nan_harness_responses_bridge(nan_harness_i18n::locale()))]
    Codex(BridgedHarnessRunArgs),
    #[command(name = "opencode", about = nan_harness_i18n::messages::help_run_opencode_through_nan_chat_completions(nan_harness_i18n::locale()))]
    OpenCode(DirectHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_hermes_agent_through_nan_chat_completions(nan_harness_i18n::locale()))]
    Hermes(DirectHarnessRunArgs),
    #[command(
        name = "hermes-desktop",
        about = nan_harness_i18n::messages::help_run_hermes_desktop_through_a_managed_nan_profile_experimental(nan_harness_i18n::locale())
    )]
    HermesDesktop(HermesDesktopArgs),
    #[command(
        name = "pen",
        visible_alias = "pen-desktop",
        about = nan_harness_i18n::messages::help_run_pen_desktop_through_a_managed_nan_model_provider_experimental(nan_harness_i18n::locale())
    )]
    PenDesktop(PenDesktopArgs),
    #[command(
        name = "zed",
        visible_alias = "zed-desktop",
        about = nan_harness_i18n::messages::help_run_zed_through_a_temporary_nan_model_provider_experimental(nan_harness_i18n::locale())
    )]
    ZedDesktop(ZedDesktopArgs),
    #[command(about = nan_harness_i18n::messages::help_run_pi_through_a_nan_provider_extension(nan_harness_i18n::locale()))]
    Pi(DirectHarnessRunArgs),
    #[command(
        name = "omp",
        visible_alias = "oh-my-pi",
        about = nan_harness_i18n::messages::help_run_oh_my_pi_through_a_nan_provider_extension(nan_harness_i18n::locale())
    )]
    Omp(DirectHarnessRunArgs),
    #[command(
        name = "prime-agent",
        visible_alias = "prime",
        about = nan_harness_i18n::messages::help_run_prime_agent_through_a_nan_provider_extension(nan_harness_i18n::locale())
    )]
    Prime(DirectHarnessRunArgs),
    #[command(
        name = "dsh",
        visible_aliases = ["deepseek", "deepseek-harness"],
        about = nan_harness_i18n::messages::help_run_deepseek_harness_through_a_temporary_nan_provider_patch(nan_harness_i18n::locale())
    )]
    DeepSeek(DirectHarnessRunArgs),
    #[command(
        name = "openclaw",
        about = nan_harness_i18n::messages::help_run_openclaw_through_a_temporary_linked_configuration(nan_harness_i18n::locale())
    )]
    OpenClaw(DirectHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_cline_through_a_temporary_linked_configuration(nan_harness_i18n::locale()))]
    Cline(DirectHarnessRunArgs),
    #[command(
        name = "qwen",
        visible_alias = "qwen-code",
        about = nan_harness_i18n::messages::help_run_qwen_code_through_nan_chat_completions(nan_harness_i18n::locale())
    )]
    Qwen(DirectHarnessRunArgs),
    #[command(
        name = "kimi",
        visible_alias = "kimi-code",
        about = nan_harness_i18n::messages::help_run_kimi_code_through_its_in_memory_nan_model_configuration(nan_harness_i18n::locale())
    )]
    Kimi(DirectHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_aider_through_nan_chat_completions(nan_harness_i18n::locale()))]
    Aider(DirectHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_goose_through_nan_chat_completions(nan_harness_i18n::locale()))]
    Goose(DirectHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_run_fx_through_the_local_nan_harness_ai_gateway_bridge(nan_harness_i18n::locale()))]
    Fx(BridgedHarnessRunArgs),
    #[command(about = nan_harness_i18n::messages::help_diagnose_nan_harness_or_inspect_one_harness_in_detail(nan_harness_i18n::locale()))]
    Doctor(DoctorArgs),
    #[command(about = nan_harness_i18n::messages::help_manage_the_saved_nan_provider_api_key(nan_harness_i18n::locale()))]
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    #[command(about = nan_harness_i18n::messages::help_configure_nan_natively_in_a_supported_harness(nan_harness_i18n::locale()))]
    Config(ConfigArgs),
    #[command(about = nan_harness_i18n::messages::help_update_nan_harness_to_the_latest_stable_release(nan_harness_i18n::locale()))]
    Update,
    #[command(about = nan_harness_i18n::messages::help_configure_and_manage_nan_web_search(nan_harness_i18n::locale()))]
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },
    #[command(about = nan_harness_i18n::messages::help_remove_nan_harness_and_its_managed_harness_integrations(nan_harness_i18n::locale()))]
    Uninstall(UninstallArgs),
    #[command(about = nan_harness_i18n::messages::help_control_anonymous_telemetry(nan_harness_i18n::locale()))]
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommand,
    },
    #[command(
        about = nan_harness_i18n::messages::help_generate_shell_completion_scripts_for_nanh(nan_harness_i18n::locale()),
        after_help = nan_harness_i18n::messages::help_load_for_the_current_session_bash_source_nanh_completions_bash_zsh_source_nanh_completions(nan_harness_i18n::locale())
    )]
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    #[command(name = "diagnostics", hide = true)]
    Diagnostics {
        #[command(subcommand)]
        command: LocalDiagnosticsCommand,
    },
    #[command(name = "__coordinator", hide = true)]
    Coordinator,
    #[command(name = "__record-installation", hide = true)]
    RecordInstallation(RecordInstallationArgs),
}
