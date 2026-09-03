mod domain;
mod error;
mod fingerprint;
mod schema;
mod validation;

pub(crate) use domain::FailureIdentity;
pub(crate) use error::ReportError;
pub(crate) use fingerprint::sha256_hex;
pub(crate) use schema::{
    CanaryObservation, CanaryObservationKind, CanaryOutcome, CanaryReport, CanaryTier,
    CanaryTrigger, CheckReport, CheckStatus, EnvironmentEvidence, FailureClass, FailureReport,
    HarnessEvidence, NanHarnessEvidence, REPORT_SCHEMA_VERSION, RuntimeEvidence,
};

#[cfg(test)]
mod tests;
