use crate::app::ConformanceArgs;
use nan_harness_test_support::conformance::{
    ConformanceError, ConformanceOutcome, PublishedConformanceRunner,
};
use thiserror::Error;

pub(crate) async fn run(arguments: &ConformanceArgs) -> Result<(), ConformanceCommandError> {
    let report =
        match PublishedConformanceRunner::new(arguments.nan_harness.clone(), arguments.harness)
            .run()
            .await
        {
            Ok(report) => report,
            Err(error) => return Err(ConformanceCommandError::Runner(error)),
        };
    if arguments.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(ConformanceCommandError::Serialize)?
        );
    } else {
        println!("{}: {:?}", report.harness, report.outcome);
    }
    if report.outcome == ConformanceOutcome::Failed {
        return Err(ConformanceCommandError::ContractFailed);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum ConformanceCommandError {
    #[error(transparent)]
    Runner(#[from] ConformanceError),
    #[error("could not serialize conformance report: {0}")]
    Serialize(serde_json::Error),
    #[error("conformance contracts failed")]
    ContractFailed,
}

#[cfg(test)]
mod tests {
    use nan_harness_core::HarnessKind;
    use nan_harness_test_support::conformance::{
        CONFORMANCE_SCHEMA_VERSION, ConformanceCheck, ConformanceOutcome, ConformanceReport,
        ConformanceScenario, ConformanceStatus,
    };
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn report_shape_contains_only_safe_contract_fields() {
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: HarnessKind::Fx,
            scenarios: vec![ConformanceScenario {
                name: "sentinel".to_owned(),
                status: ConformanceStatus::Passed,
                checks: vec![ConformanceCheck {
                    name: "contract".to_owned(),
                    status: ConformanceStatus::Passed,
                    duration_milliseconds: 0,
                }],
                duration_milliseconds: 0,
            }],
            observations: Vec::new(),
            outcome: ConformanceOutcome::Passed,
            duration_milliseconds: 0,
        };
        report
            .validate_shape()
            .expect("report shape should be bounded");
        let encoded = serde_json::to_string(&report).expect("report should serialize");
        assert!(encoded.contains("schemaVersion"));
        assert!(encoded.contains("harness"));
        assert!(encoded.contains("outcome"));
        assert!(!encoded.contains("NAN_API_KEY"));
        assert!(!encoded.contains("prompt"));
        assert!(!encoded.contains("payload"));
    }

    #[test]
    fn serialized_report_has_only_contract_fields_recursively() {
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: HarnessKind::ClaudeCode,
            scenarios: vec![ConformanceScenario {
                name: "external-prerequisite".to_owned(),
                status: ConformanceStatus::Skipped,
                checks: vec![ConformanceCheck {
                    name: "contract".to_owned(),
                    status: ConformanceStatus::Skipped,
                    duration_milliseconds: 1,
                }],
                duration_milliseconds: 1,
            }],
            observations: Vec::new(),
            outcome: ConformanceOutcome::Passed,
            duration_milliseconds: 2,
        };
        let value = serde_json::to_value(report).expect("report should serialize");
        assert_contract_object(
            &value,
            &BTreeSet::from([
                "schemaVersion",
                "harness",
                "scenarios",
                "outcome",
                "durationMilliseconds",
            ]),
        );
        let scenarios = value
            .get("scenarios")
            .and_then(Value::as_array)
            .expect("scenarios should be an array");
        assert_contract_object(
            &scenarios[0],
            &BTreeSet::from(["name", "status", "checks", "durationMilliseconds"]),
        );
        assert_contract_object(
            &scenarios[0]["checks"][0],
            &BTreeSet::from(["name", "status", "durationMilliseconds"]),
        );
        let encoded = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "path",
            "prompt",
            "tool",
            "payload",
            "result",
            "output",
            "credential",
            "diagnostic",
            "model",
        ] {
            assert!(!encoded.contains(forbidden), "report leaked {forbidden}");
        }
    }

    fn assert_contract_object(value: &Value, allowed: &BTreeSet<&str>) {
        let object = value
            .as_object()
            .expect("contract value should be an object");
        assert!(object.keys().all(|key| allowed.contains(key.as_str())));
    }
}
