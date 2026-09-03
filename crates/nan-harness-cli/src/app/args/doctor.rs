use crate::app::targets::DoctorTarget;
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub(crate) struct DoctorArgs {
    pub(crate) harness: Option<DoctorTarget>,
    #[arg(long, help = "Print a stable, safe-to-share JSON report")]
    pub(crate) json: bool,
    #[arg(long, value_name = "PATH", requires = "harness")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, requires = "harness")]
    pub(crate) allow_unsupported: bool,
    #[arg(long, requires = "harness")]
    pub(crate) allow_untested: bool,
}
