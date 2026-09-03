mod errors;
mod input;
mod persistence;
mod state;

use crate::app::AggregateArgs;
pub(crate) use errors::AggregateError;
use state::{AggregateState, AggregateSummary};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) fn run(arguments: &AggregateArgs) -> Result<(), AggregateError> {
    let reports = input::read_reports(&arguments.reports)?;
    let mut state = AggregateState::read_or_default(&arguments.state)?;
    let mut alerts = Vec::new();
    let mut processed = 0_usize;
    for report in reports {
        report.validate()?;
        if state.observe(&report, &mut alerts) {
            processed += 1;
        }
    }
    state.set_updated_at(timestamp()?);
    state.write(&arguments.state)?;

    let summary = AggregateSummary::new(&state, processed, alerts);
    persistence::atomic_json_write(&arguments.summary, &summary)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).map_err(AggregateError::Serialize)?
    );
    Ok(())
}

fn timestamp() -> Result<String, AggregateError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(AggregateError::Timestamp)
}
