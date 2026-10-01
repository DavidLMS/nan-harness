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
    #[arg(long, help = nan_harness_i18n::messages::help_print_a_stable_safe_to_share_json_report(nan_harness_i18n::locale()))]
    pub(crate) json: bool,
    #[arg(
        long,
        help = nan_harness_i18n::messages::help_check_locally_without_nan_network_activity_or_credential_store_access_harness_version_prob(nan_harness_i18n::locale())
    )]
    pub(crate) offline: bool,
    #[arg(long, value_name = "PATH", requires = "harness")]
    pub(crate) executable: Option<PathBuf>,
    #[arg(long, requires = "harness")]
    pub(crate) allow_unsupported: bool,
    #[arg(long, requires = "harness")]
    pub(crate) allow_untested: bool,
}
