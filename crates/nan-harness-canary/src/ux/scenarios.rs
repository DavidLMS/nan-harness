use super::errors::UxError;
use nan_harness_diagnostics::{MessageLevel, ReportPolicy, UserMessage};
use serde::Deserialize;
use std::collections::BTreeSet;

const SCENARIOS: &str = include_str!("../../resources/ux-scenarios.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UxScenario {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) category: String,
    pub(super) command: String,
    pub(super) appears_when: String,
    pub(super) result: String,
    #[serde(default)]
    pub(super) terminal_output: Option<String>,
    pub(super) message: UserMessage,
}

pub(super) fn load_scenarios() -> Result<Vec<UxScenario>, UxError> {
    let scenarios: Vec<UxScenario> = serde_json::from_str(SCENARIOS).map_err(UxError::Parse)?;
    validate_scenarios(&scenarios)?;
    Ok(scenarios)
}

fn validate_scenarios(scenarios: &[UxScenario]) -> Result<(), UxError> {
    let mut identifiers = BTreeSet::new();
    for scenario in scenarios {
        validate_unique_identifier(scenario, &mut identifiers)?;
        validate_required_fields(scenario)?;
        validate_terminal_output(scenario)?;
        validate_message(scenario)?;
    }
    Ok(())
}

fn validate_unique_identifier<'a>(
    scenario: &'a UxScenario,
    identifiers: &mut BTreeSet<&'a str>,
) -> Result<(), UxError> {
    if identifiers.insert(scenario.id.as_str()) {
        Ok(())
    } else {
        Err(UxError::DuplicateScenario(scenario.id.clone()))
    }
}

fn validate_required_fields(scenario: &UxScenario) -> Result<(), UxError> {
    if scenario.id.trim().is_empty()
        || scenario.title.trim().is_empty()
        || scenario.category.trim().is_empty()
        || scenario.command.trim().is_empty()
        || scenario.appears_when.trim().is_empty()
        || scenario.result.trim().is_empty()
        || scenario.message.summary.trim().is_empty()
    {
        Err(UxError::InvalidScenario(scenario.id.clone()))
    } else {
        Ok(())
    }
}

fn validate_terminal_output(scenario: &UxScenario) -> Result<(), UxError> {
    if scenario
        .terminal_output
        .as_deref()
        .is_some_and(|output| output.trim().is_empty())
    {
        Err(UxError::InvalidScenario(scenario.id.clone()))
    } else {
        Ok(())
    }
}

fn validate_message(scenario: &UxScenario) -> Result<(), UxError> {
    let valid = match scenario.message.level {
        MessageLevel::Error => {
            scenario.message.code.is_some()
                && scenario.message.report_policy == ReportPolicy::ConsentAware
        }
        MessageLevel::Warning | MessageLevel::SetupRequired => {
            scenario.message.code.is_none() && scenario.message.report_policy == ReportPolicy::Never
        }
    };
    valid
        .then_some(())
        .ok_or_else(|| UxError::InvalidScenario(scenario.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::{UxError, UxScenario, load_scenarios, validate_scenarios};
    use nan_harness_diagnostics::{MessageLevel, ReportPolicy, UserMessage};

    fn valid_scenario() -> UxScenario {
        UxScenario {
            id: "scenario".to_owned(),
            title: "Scenario".to_owned(),
            category: "category".to_owned(),
            command: "nanh test".to_owned(),
            appears_when: "always".to_owned(),
            result: "the result".to_owned(),
            terminal_output: None,
            message: UserMessage::warning("summary"),
        }
    }

    #[test]
    fn embedded_scenarios_are_valid_and_cover_setup_and_errors() {
        let scenarios = load_scenarios().expect("embedded scenarios should be valid");
        assert!(scenarios.len() >= 23);
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "deepseek-node-old")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "tool-bridge-failed")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "native-config-user-change")
        );
    }

    #[test]
    fn scenario_validation_rejects_duplicate_identifiers_before_other_errors() {
        let mut duplicate = valid_scenario();
        duplicate.title.clear();
        let scenarios = [valid_scenario(), duplicate];

        assert!(matches!(
            validate_scenarios(&scenarios),
            Err(UxError::DuplicateScenario(id)) if id == "scenario"
        ));
    }

    #[test]
    fn scenario_validation_rejects_empty_required_fields_and_terminal_output() {
        for field in [
            "id",
            "title",
            "category",
            "command",
            "appears_when",
            "result",
            "summary",
        ] {
            let mut scenario = valid_scenario();
            match field {
                "id" => scenario.id.clear(),
                "title" => scenario.title.clear(),
                "category" => scenario.category.clear(),
                "command" => scenario.command.clear(),
                "appears_when" => scenario.appears_when.clear(),
                "result" => scenario.result.clear(),
                "summary" => scenario.message.summary.clear(),
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    validate_scenarios(&[scenario]),
                    Err(UxError::InvalidScenario(_))
                ),
                "empty {field} should be rejected"
            );
        }

        let mut scenario = valid_scenario();
        scenario.terminal_output = Some("  \n".to_owned());
        assert!(matches!(
            validate_scenarios(&[scenario]),
            Err(UxError::InvalidScenario(_))
        ));
    }

    #[test]
    fn scenario_validation_accepts_only_the_declared_message_combinations() {
        for (level, code, report_policy, valid) in [
            (
                MessageLevel::Error,
                Some("NH-TEST".to_owned()),
                ReportPolicy::ConsentAware,
                true,
            ),
            (MessageLevel::Error, None, ReportPolicy::ConsentAware, false),
            (
                MessageLevel::Error,
                Some("NH-TEST".to_owned()),
                ReportPolicy::Never,
                false,
            ),
            (MessageLevel::Warning, None, ReportPolicy::Never, true),
            (
                MessageLevel::Warning,
                Some("NH-TEST".to_owned()),
                ReportPolicy::Never,
                false,
            ),
            (
                MessageLevel::Warning,
                None,
                ReportPolicy::ConsentAware,
                false,
            ),
            (MessageLevel::SetupRequired, None, ReportPolicy::Never, true),
            (
                MessageLevel::SetupRequired,
                Some("NH-TEST".to_owned()),
                ReportPolicy::Never,
                false,
            ),
            (
                MessageLevel::SetupRequired,
                None,
                ReportPolicy::ConsentAware,
                false,
            ),
        ] {
            let mut scenario = valid_scenario();
            scenario.message.level = level;
            scenario.message.code = code;
            scenario.message.report_policy = report_policy;
            assert_eq!(
                validate_scenarios(&[scenario]).is_ok(),
                valid,
                "unexpected validation result for {level:?}"
            );
        }
    }
}
