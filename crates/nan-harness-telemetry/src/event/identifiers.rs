use std::fmt::Write as _;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(super) fn generate_report_id() -> Result<String, EventError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(EventError::Random)?;
    let mut identifier = String::with_capacity(39);
    identifier.push_str("report_");
    for byte in bytes {
        write!(&mut identifier, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(identifier)
}

pub(super) fn timestamp(value: OffsetDateTime) -> Result<String, EventError> {
    value.format(&Rfc3339).map_err(EventError::Timestamp)
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("could not generate an error report identifier: {0}")]
    Random(getrandom::Error),
    #[error("could not format the error report timestamp: {0}")]
    Timestamp(time::error::Format),
}
