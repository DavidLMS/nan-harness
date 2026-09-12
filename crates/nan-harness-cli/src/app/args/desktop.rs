use super::launch::{HarnessRunArgs, WebSearchArgs};
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ChatGptDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "MODEL")]
    pub(crate) aux_model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_retained_for_compatibility_newer_desktop_versions_warn_and_continue(nan_harness_i18n::locale())
    )]
    pub(crate) allow_untested: bool,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = nan_harness_i18n::messages::help_show_verbose_potentially_private_chatgpt_desktop_logs(nan_harness_i18n::locale()))]
    pub(crate) debug: bool,
    #[arg(long, help = nan_harness_i18n::messages::help_print_the_inert_launch_plan_without_changing_state(nan_harness_i18n::locale()))]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_limit_the_total_provider_input_and_output_tokens_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) session_max_tokens: Option<u64>,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_set_the_native_compaction_target_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) context: Option<u64>,
    #[arg(
        long,
        value_name = "SECONDS",
        value_parser = clap::value_parser!(u64).range(1..=86_400),
        help = nan_harness_i18n::messages::help_fail_if_the_app_does_not_connect_within_this_many_seconds(nan_harness_i18n::locale())
    )]
    pub(crate) startup_timeout: Option<u64>,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_restore_receipt_backed_state_from_an_interrupted_launch(nan_harness_i18n::locale()),
        conflicts_with_all = ["model", "aux_model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "no_search", "force_search", "debug", "dry_run", "session_max_tokens", "context", "startup_timeout"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClaudeDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_retained_for_compatibility_newer_desktop_versions_warn_and_continue(nan_harness_i18n::locale())
    )]
    pub(crate) allow_untested: bool,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = nan_harness_i18n::messages::help_print_the_inert_launch_plan_without_changing_state(nan_harness_i18n::locale()))]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_limit_the_total_provider_input_and_output_tokens_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) session_max_tokens: Option<u64>,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_set_the_native_compaction_target_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) context: Option<u64>,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_show_auto_requests_and_responses_that_may_contain_private_data(nan_harness_i18n::locale())
    )]
    pub(crate) show_auto: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_restore_receipt_backed_state_from_an_interrupted_launch(nan_harness_i18n::locale()),
        conflicts_with_all = ["model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "no_search", "force_search", "dry_run", "session_max_tokens", "context", "show_auto"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
pub(crate) struct HermesDesktopArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(long, help = nan_harness_i18n::messages::help_bypass_the_local_gateway_in_a_diagnostic_profile(nan_harness_i18n::locale()))]
    pub(crate) no_chat_gateway: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_restore_receipt_backed_state_from_an_interrupted_launch(nan_harness_i18n::locale()),
        conflicts_with_all = ["model", "executable", "provider_base_url", "allow_unsupported", "allow_untested", "no_search", "force_search", "dry_run", "session_max_tokens", "context", "no_chat_gateway", "arguments"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct PenDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_retained_for_compatibility_newer_desktop_versions_warn_and_continue(nan_harness_i18n::locale())
    )]
    pub(crate) allow_untested: bool,
    #[arg(long, help = nan_harness_i18n::messages::help_print_the_inert_launch_plan_without_changing_state(nan_harness_i18n::locale()))]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_limit_the_total_provider_input_and_output_tokens_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) session_max_tokens: Option<u64>,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_set_the_native_compaction_target_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) context: Option<u64>,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_restore_receipt_backed_state_from_an_interrupted_launch(nan_harness_i18n::locale()),
        conflicts_with_all = ["model", "provider_base_url", "executable", "allow_unsupported", "allow_untested", "dry_run", "session_max_tokens", "context"]
    )]
    pub(crate) restore: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ZedDesktopArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_retained_for_compatibility_newer_desktop_versions_warn_and_continue(nan_harness_i18n::locale())
    )]
    pub(crate) allow_untested: bool,
    #[arg(long, help = nan_harness_i18n::messages::help_print_the_inert_launch_plan_without_changing_state(nan_harness_i18n::locale()))]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_limit_the_total_provider_input_and_output_tokens_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) session_max_tokens: Option<u64>,
    #[arg(
        long,
        value_name = "TOKENS",
        value_parser = clap::value_parser!(u64).range(1..),
        help = nan_harness_i18n::messages::help_set_the_native_compaction_target_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) context: Option<u64>,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_restore_receipt_backed_state_from_an_interrupted_launch(nan_harness_i18n::locale()),
        conflicts_with_all = ["model", "executable", "allow_unsupported", "allow_untested", "dry_run", "session_max_tokens", "context", "workspace", "arguments"]
    )]
    pub(crate) restore: bool,
    #[arg(value_name = "WORKSPACE", conflicts_with = "restore")]
    pub(crate) workspace: Option<PathBuf>,
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        conflicts_with = "restore"
    )]
    pub(crate) arguments: Vec<String>,
}
