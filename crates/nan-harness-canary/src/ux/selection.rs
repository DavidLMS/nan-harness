use super::{errors::UxError, scenarios::UxScenario};

pub(super) fn select_scenarios<'a>(
    scenarios: &'a [UxScenario],
    identifier: Option<&str>,
) -> Result<Vec<&'a UxScenario>, UxError> {
    let Some(identifier) = identifier else {
        return Ok(scenarios.iter().collect());
    };
    scenarios
        .iter()
        .find(|scenario| scenario.id == identifier)
        .map(|scenario| vec![scenario])
        .ok_or_else(|| UxError::UnknownScenario(identifier.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{UxError, select_scenarios};
    use crate::ux::scenarios::load_scenarios;

    #[test]
    fn selection_preserves_all_scenarios_and_filters_by_identifier() {
        let scenarios = load_scenarios().expect("embedded scenarios should be valid");

        let all = select_scenarios(&scenarios, None).expect("no filter should select all");
        assert_eq!(all.len(), scenarios.len());

        let selected = select_scenarios(&scenarios, Some("deepseek-node-old"))
            .expect("known identifier should select one scenario");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "deepseek-node-old");
    }

    #[test]
    fn selection_rejects_unknown_identifiers() {
        let scenarios = load_scenarios().expect("embedded scenarios should be valid");

        assert!(matches!(
            select_scenarios(&scenarios, Some("missing")),
            Err(UxError::UnknownScenario(id)) if id == "missing"
        ));
    }
}
