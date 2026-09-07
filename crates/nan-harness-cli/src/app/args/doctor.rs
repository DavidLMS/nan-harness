use crate::app::targets::DoctorTarget;
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
// Four independent CLI switches, not mutually exclusive states. Revisit if their semantics couple.
#[expect(
    clippy::struct_excessive_bools,
    reason = "preserve independent clap switches"
)]
pub(crate) struct DoctorArgs {
    pub(crate) harness: Option<DoctorTarget>,
    #[arg(long, help = "Print a stable, safe-to-share JSON report")]
    pub(crate) json: bool,
    #[arg(
        long,
        help = "Check locally without NaN network activity or credential-store access; harness version probes still run"
    )]
    pub(crate) offline: bool,
    #[arg(long, value_name = "PATH", requires = "harness")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, requires = "harness")]
    pub(crate) allow_unsupported: bool,
    #[arg(long, requires = "harness")]
    pub(crate) allow_untested: bool,
}
