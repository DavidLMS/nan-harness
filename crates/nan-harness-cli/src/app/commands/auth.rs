use super::super::args::AuthLogoutArgs;
use clap::Subcommand;

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum AuthCommand {
    #[command(about = nan_harness_i18n::messages::help_verify_and_save_a_nan_api_key(nan_harness_i18n::locale()))]
    Login,
    #[command(about = nan_harness_i18n::messages::help_show_where_the_active_nan_api_key_comes_from(nan_harness_i18n::locale()))]
    Status,
    #[command(about = nan_harness_i18n::messages::help_remove_the_api_key_previously_saved_by_nan_harness(nan_harness_i18n::locale()))]
    Logout(AuthLogoutArgs),
}
