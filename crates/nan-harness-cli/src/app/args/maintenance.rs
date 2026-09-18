use clap::Args;
use std::path::PathBuf;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum SearchCommand {
    #[command(about = nan_harness_i18n::messages::help_configure_and_when_selected_install_a_searxng_backend(nan_harness_i18n::locale()))]
    Setup(SearchSetupArgs),
    #[command(about = nan_harness_i18n::messages::help_show_the_configured_searxng_backend_without_starting_it(nan_harness_i18n::locale()))]
    Status(SearchStatusArgs),
    #[command(about = nan_harness_i18n::messages::help_disable_nan_web_search_without_removing_its_backend(nan_harness_i18n::locale()))]
    Disable,
    #[command(about = nan_harness_i18n::messages::help_update_the_configured_managed_searxng_backend(nan_harness_i18n::locale()))]
    Update,
    #[command(about = nan_harness_i18n::messages::help_remove_the_configured_backend_and_its_saved_endpoint(nan_harness_i18n::locale()))]
    Remove,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("backend")
        .multiple(false)
        .args(["local", "docker", "url"])
))]
pub(crate) struct SearchSetupArgs {
    #[arg(long, help = nan_harness_i18n::messages::help_install_and_supervise_a_private_local_searxng_instance(nan_harness_i18n::locale()))]
    pub(crate) local: bool,
    #[arg(long, help = nan_harness_i18n::messages::help_create_and_manage_a_searxng_docker_container(nan_harness_i18n::locale()))]
    pub(crate) docker: bool,
    #[arg(
        long,
        value_name = "URL",
        help = nan_harness_i18n::messages::help_use_an_https_searxng_endpoint_managed_elsewhere(nan_harness_i18n::locale())
    )]
    pub(crate) url: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct SearchStatusArgs {
    #[arg(long, help = nan_harness_i18n::messages::help_render_machine_readable_json_status(nan_harness_i18n::locale()))]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct UninstallArgs {
    #[arg(short = 'y', long, help = nan_harness_i18n::messages::help_uninstall_without_asking_for_confirmation(nan_harness_i18n::locale()))]
    pub(crate) yes: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RecordInstallationArgs {
    #[arg(long, value_name = "PATH")]
    pub(crate) executable: PathBuf,
    #[arg(long, value_name = "PATH")]
    pub(crate) alias: PathBuf,
    #[arg(long)]
    pub(crate) user_path_entry_added: bool,
}
