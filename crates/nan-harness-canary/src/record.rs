use crate::app::RecordArgs;
use crate::report::{
    CanaryOutcome, CanaryReport, CheckReport, CheckStatus, EnvironmentEvidence, FailureClass,
    FailureIdentity, FailureReport, HarnessEvidence, NanHarnessEvidence, REPORT_SCHEMA_VERSION,
};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) fn run(arguments: &RecordArgs) -> Result<(), RecordError> {
    let failure = match (
        arguments.passed,
        arguments.failure_class,
        arguments.failure_phase.as_deref(),
        arguments.failure_summary.as_deref(),
    ) {
        (true, None, None, None) => None,
        (false, Some(class), Some(phase), Some(summary)) => Some(FailureReport::new(
            class,
            phase,
            None,
            summary,
            &FailureIdentity {
                harness: arguments.harness,
                harness_version: &arguments.harness_version,
                operating_system: &arguments.operating_system,
                architecture: &arguments.architecture,
                tier: arguments.tier,
                scenario: &arguments.scenario,
            },
        )),
        _ => return Err(RecordError::InvalidOutcome),
    };
    let outcome = failure.as_ref().map_or(CanaryOutcome::Passed, |failure| {
        if failure.class == FailureClass::Infrastructure {
            CanaryOutcome::InfrastructureFailure
        } else {
            CanaryOutcome::Failed
        }
    });
    let now = timestamp()?;
    let report = CanaryReport {
        schema_version: REPORT_SCHEMA_VERSION,
        run_id: arguments.run_id.clone(),
        cell_id: arguments.cell_id.clone(),
        spec_sha256: arguments.spec_sha256.clone(),
        trigger: arguments.trigger,
        tier: arguments.tier,
        scenario: arguments.scenario.clone(),
        started_at: now.clone(),
        completed_at: now,
        duration_milliseconds: arguments.duration_milliseconds,
        nan_harness: NanHarnessEvidence {
            version: arguments.nan_harness_version.clone(),
            source: arguments.nan_harness_source.clone(),
            sha256: arguments.nan_harness_sha256.clone(),
        },
        environment: EnvironmentEvidence {
            operating_system: arguments.operating_system.clone(),
            architecture: arguments.architecture.clone(),
            image: arguments.image.clone(),
            profile: arguments.profile.clone(),
            runtimes: Vec::new(),
        },
        harness: HarnessEvidence {
            id: arguments.harness,
            version: arguments.harness_version.clone(),
        },
        model: arguments.model.clone(),
        checks: vec![CheckReport {
            name: arguments.check.clone(),
            status: if arguments.passed {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            },
            duration_milliseconds: arguments.duration_milliseconds,
            attempts: 1,
            detail: None,
        }],
        outcome,
        failure,
    };
    report.write(&arguments.output)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn timestamp() -> Result<String, RecordError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(RecordError::Timestamp)
}

#[derive(Debug, Error)]
pub(crate) enum RecordError {
    #[error("record requires either --passed or complete failure metadata, but never both")]
    InvalidOutcome,
    #[error("could not format a report timestamp: {0}")]
    Timestamp(time::error::Format),
    #[error(transparent)]
    Report(#[from] crate::report::ReportError),
    #[error("could not serialize the report: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::RecordError;
    use crate::app::RecordArgs;
    use crate::report::{CanaryTier, CanaryTrigger};
    use nan_harness_core::HarnessKind;

    #[test]
    fn passed_records_reject_failure_metadata() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let arguments = RecordArgs {
            output: directory.path().join("report.json"),
            run_id: "run".to_owned(),
            cell_id: "cell".to_owned(),
            spec_sha256: "a".repeat(64),
            trigger: CanaryTrigger::Daily,
            tier: CanaryTier::Deterministic,
            scenario: "inventory".to_owned(),
            nan_harness_version: "0.0.6".to_owned(),
            nan_harness_source: "commit".to_owned(),
            nan_harness_sha256: "b".repeat(64),
            operating_system: "linux".to_owned(),
            architecture: "x86_64".to_owned(),
            image: "ubuntu-24.04".to_owned(),
            profile: "node-24".to_owned(),
            harness: HarnessKind::Codex,
            harness_version: "0.146.0".to_owned(),
            model: None,
            check: "inventory".to_owned(),
            duration_milliseconds: 1,
            passed: true,
            failure_class: Some(crate::report::FailureClass::Harness),
            failure_phase: Some("inventory".to_owned()),
            failure_summary: Some("failed".to_owned()),
        };

        assert!(matches!(
            super::run(&arguments),
            Err(RecordError::InvalidOutcome)
        ));
    }
}
