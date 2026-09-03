use super::errors::AggregateError;
use crate::report::CanaryReport;
use std::fs;
use std::path::Path;

pub(super) fn read_reports(directory: &Path) -> Result<Vec<CanaryReport>, AggregateError> {
    let entries = fs::read_dir(directory).map_err(|source| AggregateError::ReadDirectory {
        path: directory.to_owned(),
        source,
    })?;
    let mut reports = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| AggregateError::ReadDirectory {
            path: directory.to_owned(),
            source,
        })?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            reports.push(CanaryReport::read(&path)?);
        }
    }
    if reports.is_empty() {
        return Err(AggregateError::NoReports(directory.to_owned()));
    }
    reports.sort_by(|left, right| {
        left.completed_at
            .cmp(&right.completed_at)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::read_reports;
    use crate::aggregate::errors::AggregateError;
    use crate::report::{
        CanaryOutcome, CanaryReport, CanaryTier, CanaryTrigger, CheckReport, CheckStatus,
        EnvironmentEvidence, HarnessEvidence, NanHarnessEvidence, REPORT_SCHEMA_VERSION,
    };
    use nan_harness_core::HarnessKind;

    fn report(run_id: &str, completed_at: &str) -> CanaryReport {
        CanaryReport {
            schema_version: REPORT_SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            cell_id: "linux-kimi-deterministic-edit".to_owned(),
            spec_sha256: "b".repeat(64),
            trigger: CanaryTrigger::Manual,
            tier: CanaryTier::Deterministic,
            scenario: "edit".to_owned(),
            started_at: "2026-08-22T08:00:00Z".to_owned(),
            completed_at: completed_at.to_owned(),
            duration_milliseconds: 1_000,
            nan_harness: NanHarnessEvidence {
                version: "0.0.18".to_owned(),
                source: "release".to_owned(),
                sha256: "a".repeat(64),
            },
            environment: EnvironmentEvidence {
                operating_system: "linux".to_owned(),
                architecture: "aarch64".to_owned(),
                image: "ubuntu".to_owned(),
                profile: "node-24".to_owned(),
                runtimes: Vec::new(),
            },
            harness: HarnessEvidence {
                id: HarnessKind::KimiCode,
                version: "1.2.3".to_owned(),
            },
            model: None,
            checks: vec![CheckReport {
                name: "tool-edit".to_owned(),
                status: CheckStatus::Passed,
                duration_milliseconds: 1_000,
                attempts: 1,
                detail: None,
            }],
            observations: Vec::new(),
            outcome: CanaryOutcome::Passed,
            failure: None,
        }
    }

    #[test]
    fn non_json_entries_do_not_count_as_reports() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        std::fs::write(directory.path().join("notes.txt"), b"not a report")
            .expect("non-report should be written");

        let error = read_reports(directory.path()).expect_err("directory should have no reports");
        assert!(matches!(error, AggregateError::NoReports(path) if path == directory.path()));
    }

    #[test]
    fn reports_are_ordered_by_completion_then_run_id() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        for (file_name, report) in [
            ("first.json", report("run-c", "2026-08-22T08:00:02Z")),
            ("second.json", report("run-b", "2026-08-22T08:00:01Z")),
            ("third.json", report("run-a", "2026-08-22T08:00:01Z")),
        ] {
            report
                .write(&directory.path().join(file_name))
                .expect("report should be written");
        }

        let reports = read_reports(directory.path()).expect("reports should be read");
        assert_eq!(
            reports
                .iter()
                .map(|report| report.run_id.as_str())
                .collect::<Vec<_>>(),
            ["run-a", "run-b", "run-c"]
        );
    }
}
