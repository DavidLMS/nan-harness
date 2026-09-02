use super::{errors::CellError, spec::LoadedSpec, workspace::CellWorkspace};
use crate::report::{
    CanaryObservation, CanaryOutcome, CanaryReport, CheckReport, CheckStatus, EnvironmentEvidence,
    FailureClass, FailureIdentity, FailureReport, HarnessEvidence, NanHarnessEvidence,
    REPORT_SCHEMA_VERSION,
};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) struct ExecutionTiming {
    pub(crate) started_at: String,
    pub(crate) completed_at: String,
    pub(crate) duration: Duration,
}

pub(crate) fn preserve_private_logs(
    workspace: &CellWorkspace,
    private_log_directory: Option<&Path>,
    execution: Result<Vec<CheckReport>, RuntimeFailure>,
) -> Result<Vec<CheckReport>, RuntimeFailure> {
    let Some(directory) = private_log_directory else {
        return execution;
    };
    if workspace.preserve_logs(directory).is_ok() {
        return execution;
    }
    let check = failed_check(
        "preserve-private-logs",
        Duration::ZERO,
        1,
        "private diagnostic logs could not be preserved",
    );
    match execution {
        Ok(mut checks) => {
            checks.push(check);
            Err(RuntimeFailure::new(
                FailureClass::Infrastructure,
                "preserve-private-logs",
                "private diagnostic logs could not be preserved",
                checks,
            ))
        }
        Err(mut failure) => {
            failure.checks.push(check);
            Err(failure)
        }
    }
}

pub(crate) fn build_report(
    loaded: LoadedSpec,
    workspace: CellWorkspace,
    model: Option<String>,
    harness_version: String,
    observations: Vec<CanaryObservation>,
    timing: ExecutionTiming,
    execution: Result<Vec<CheckReport>, RuntimeFailure>,
) -> CanaryReport {
    let (checks, failure) = match execution {
        Ok(checks) => (checks, None),
        Err(runtime_failure) => {
            let identity = FailureIdentity {
                harness: loaded.value.harness,
                harness_version: &harness_version,
                operating_system: loaded.value.guest.as_str(),
                architecture: "aarch64",
                tier: loaded.value.tier,
                scenario: &loaded.value.scenario,
            };
            let report = FailureReport::new(
                runtime_failure.class,
                runtime_failure.phase,
                None,
                runtime_failure.summary,
                &identity,
            );
            (runtime_failure.checks, Some(report))
        }
    };
    let outcome = failure.as_ref().map_or(CanaryOutcome::Passed, |failure| {
        if failure.class == FailureClass::Infrastructure {
            CanaryOutcome::InfrastructureFailure
        } else {
            CanaryOutcome::Failed
        }
    });

    CanaryReport {
        schema_version: REPORT_SCHEMA_VERSION,
        run_id: run_id(&loaded.value.id),
        cell_id: loaded.value.id,
        spec_sha256: loaded.sha256,
        trigger: loaded.value.trigger,
        tier: loaded.value.tier,
        scenario: loaded.value.scenario,
        started_at: timing.started_at,
        completed_at: timing.completed_at,
        duration_milliseconds: milliseconds(timing.duration),
        nan_harness: NanHarnessEvidence {
            version: loaded.value.nan_harness.version,
            source: loaded.value.nan_harness.source,
            sha256: workspace.nan_harness_sha256,
        },
        environment: EnvironmentEvidence {
            operating_system: loaded.value.guest.as_str().to_owned(),
            architecture: "aarch64".to_owned(),
            image: loaded.value.image,
            profile: loaded.value.profile,
            runtimes: loaded.value.runtimes,
        },
        harness: HarnessEvidence {
            id: loaded.value.harness,
            version: harness_version,
        },
        model,
        checks,
        observations,
        outcome,
        failure,
    }
}

pub(crate) struct RuntimeFailure {
    pub(crate) class: FailureClass,
    pub(crate) phase: String,
    pub(crate) summary: String,
    pub(crate) checks: Vec<CheckReport>,
}

impl RuntimeFailure {
    pub(crate) fn new(
        class: FailureClass,
        phase: impl Into<String>,
        summary: impl Into<String>,
        checks: Vec<CheckReport>,
    ) -> Self {
        Self {
            class,
            phase: phase.into(),
            summary: summary.into(),
            checks,
        }
    }
}

pub(crate) fn passed_check(name: &str, duration: Duration, attempts: u8) -> CheckReport {
    CheckReport {
        name: name.to_owned(),
        status: CheckStatus::Passed,
        duration_milliseconds: milliseconds(duration),
        attempts,
        detail: None,
    }
}

pub(crate) fn failed_check(
    name: &str,
    duration: Duration,
    attempts: u8,
    detail: &str,
) -> CheckReport {
    CheckReport {
        name: name.to_owned(),
        status: CheckStatus::Failed,
        duration_milliseconds: milliseconds(duration),
        attempts,
        detail: Some(detail.to_owned()),
    }
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn timestamp() -> Result<String, CellError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(CellError::Timestamp)
}

fn run_id(cell_id: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{cell_id}-{nanos}-{}", std::process::id())
}
