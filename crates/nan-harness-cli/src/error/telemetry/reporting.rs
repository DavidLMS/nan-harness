use super::super::CliError;
use crate::app::Cli;
use crate::observability::is_harness_dry_run;
use nan_harness_core::PlanError;
use nan_harness_runtime::DiscoveryError;
use nan_harness_runtime::update::UpdateError;

pub(super) fn should_report(error: &CliError, cli: &Cli) -> bool {
    if matches!(
        error,
        CliError::Update(UpdateError::UpdateChannelUnavailable) | CliError::UsageEvidence(_)
    ) {
        return false;
    }

    if is_harness_dry_run(cli)
        && matches!(
            error,
            CliError::Discovery(DiscoveryError::InvalidExecutable(_))
                | CliError::InvalidPlan(PlanError::InvalidField { .. })
        )
    {
        return false;
    }

    true
}
