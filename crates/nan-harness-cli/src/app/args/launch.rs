use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub(crate) struct HarnessRunArgs {
    #[arg(long)]
    pub(crate) model: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, value_name = "URL")]
    pub(crate) provider_base_url: Option<String>,
    #[arg(long)]
    pub(crate) allow_unsupported: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_allow_an_unreadable_harness_version_newer_versions_already_warn_and_continue(nan_harness_i18n::locale())
    )]
    pub(crate) allow_untested: bool,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(long, help = nan_harness_i18n::messages::help_print_the_safe_launch_plan_without_starting_the_harness(nan_harness_i18n::locale()))]
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
        help = nan_harness_i18n::messages::help_set_the_native_compaction_target_for_this_harness(nan_harness_i18n::locale())
    )]
    pub(crate) context: Option<u64>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) arguments: Vec<String>,
}

#[derive(Debug, Default, Args)]
pub(crate) struct WebSearchArgs {
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_do_not_add_nan_web_search_preserve_any_existing_search_configuration(nan_harness_i18n::locale()),
        conflicts_with = "force_search"
    )]
    pub(crate) no_search: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_use_nan_web_search_even_when_another_search_provider_is_configured(nan_harness_i18n::locale()),
        conflicts_with = "no_search"
    )]
    pub(crate) force_search: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DirectHarnessRunArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_bypass_the_local_chat_completions_gateway_for_this_launch(nan_harness_i18n::locale())
    )]
    pub(crate) no_chat_gateway: bool,
}

#[derive(Debug, Args)]
pub(crate) struct BridgedHarnessRunArgs {
    #[command(flatten)]
    pub(crate) run: HarnessRunArgs,
    #[arg(long, hide = true)]
    pub(crate) no_chat_gateway: bool,
}
