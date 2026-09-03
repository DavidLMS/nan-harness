use super::super::args::AuthLogoutArgs;
use clap::Subcommand;

#[derive(Debug, Clone, Copy, Subcommand)]
#[command(disable_help_subcommand = true)]
pub(crate) enum AuthCommand {
    #[command(about = "Verify and save a NaN API key")]
    Login,
    #[command(about = "Show where the active NaN API key comes from")]
    Status,
    #[command(about = "Remove the API key previously saved by nan-harness")]
    Logout(AuthLogoutArgs),
}
