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
        CONFORMANCE_SCHEMA_VERSION, ConformanceOutcome, ConformanceReport,
    };

    #[test]
    fn report_shape_contains_only_safe_contract_fields() {
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: HarnessKind::Fx,
            scenarios: Vec::new(),
            outcome: ConformanceOutcome::Passed,
            duration_milliseconds: 0,
        };
        let encoded = serde_json::to_string(&report).expect("report should serialize");
        assert!(encoded.contains("schemaVersion"));
        assert!(encoded.contains("harness"));
        assert!(encoded.contains("outcome"));
        assert!(!encoded.contains("NAN_API_KEY"));
        assert!(!encoded.contains("prompt"));
        assert!(!encoded.contains("payload"));
    }
}
