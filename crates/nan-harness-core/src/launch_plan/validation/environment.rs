use super::invalid;
use crate::error::PlanError;
use crate::launch_plan::LaunchPlan;
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;

pub(super) fn validate(plan: &LaunchPlan) -> Result<(), PlanError> {
    for variable in plan
        .environment
        .public
        .keys()
        .chain(plan.environment.secrets.keys())
        .chain(plan.environment.remove.iter())
    {
        if !is_valid_environment_name(variable) {
            return invalid(
                "environment",
                DiagnosticText::new(|locale| {
                    detail_messages::detail_variable_is_not_a_valid_variable_name(
                        locale,
                        &(variable),
                    )
                }),
            );
        }
    }

    for variable in plan.environment.public.keys() {
        if plan.environment.secrets.contains_key(variable)
            || plan.environment.remove.contains(variable)
        {
            return Err(PlanError::ConflictingEnvironment {
                variable: variable.clone(),
            });
        }
    }
    for variable in plan.environment.secrets.keys() {
        if plan.environment.remove.contains(variable) {
            return Err(PlanError::ConflictingEnvironment {
                variable: variable.clone(),
            });
        }
        if !plan
            .observability
            .redact_environment_names
            .contains(variable)
        {
            return invalid(
                "observability.redactEnvironmentNames",
                DiagnosticText::new(|locale| {
                    detail_messages::detail_must_include_secret_environment_variable_variable(
                        locale,
                        &(variable),
                    )
                }),
            );
        }
    }
    Ok(())
}

fn is_valid_environment_name(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}
