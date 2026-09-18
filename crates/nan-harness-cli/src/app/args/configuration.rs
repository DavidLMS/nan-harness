use super::launch::WebSearchArgs;
use crate::app::targets::{ConfigTarget, parse_config_harness};
use clap::Args;

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ConfigArgs {
    #[arg(
        value_name = "HARNESS",
        help = nan_harness_i18n::messages::help_harness_whose_native_user_configuration_should_be_managed(nan_harness_i18n::locale()),
        value_parser = parse_config_harness
    )]
    pub(crate) harness: Option<ConfigTarget>,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_inspect_one_harness_or_all_harnesses_when_harness_is_omitted(nan_harness_i18n::locale()),
        conflicts_with_all = ["refresh", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) status: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_refresh_the_copied_key_model_catalog_and_managed_defaults(nan_harness_i18n::locale()),
        requires = "harness",
        conflicts_with_all = ["status", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) refresh: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_remove_this_managed_native_configuration_safely(nan_harness_i18n::locale()),
        requires = "harness",
        conflicts_with_all = ["status", "refresh", "refresh_all", "remove_all"]
    )]
    pub(crate) remove: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_refresh_every_native_configuration_managed_by_nan_harness(nan_harness_i18n::locale()),
        conflicts_with_all = ["harness", "status", "refresh", "remove", "remove_all"]
    )]
    pub(crate) refresh_all: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_remove_every_native_configuration_managed_by_nan_harness(nan_harness_i18n::locale()),
        conflicts_with_all = ["harness", "status", "refresh", "remove", "refresh_all"]
    )]
    pub(crate) remove_all: bool,
    #[arg(
        short = 'y',
        long,
        help = nan_harness_i18n::messages::help_confirm_first_time_configuration_or_remove_all_without_prompting(nan_harness_i18n::locale())
    )]
    pub(crate) yes: bool,
}
