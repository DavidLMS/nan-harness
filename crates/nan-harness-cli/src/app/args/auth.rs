use clap::Args;

#[derive(Debug, Clone, Copy, Args)]
pub(crate) struct AuthLogoutArgs {
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_remove_managed_harness_configurations_before_deleting_the_saved_key(nan_harness_i18n::locale()),
        conflicts_with = "keep_configs"
    )]
    pub(crate) remove_configs: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_keep_managed_harness_configurations_and_their_copied_keys(nan_harness_i18n::locale()),
        conflicts_with = "remove_configs"
    )]
    pub(crate) keep_configs: bool,
    #[arg(
        short = 'y',
        long,
        help = nan_harness_i18n::messages::help_confirm_the_selected_logout_behavior_without_prompting(nan_harness_i18n::locale())
    )]
    pub(crate) yes: bool,
}
