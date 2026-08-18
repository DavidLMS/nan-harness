use nan_harness_core::launch_plan::{
    CleanupPolicy, ConfigurationOverlay, EnvironmentOverlay, ObservabilityPolicy,
    PROVIDER_BASE_URL_PLACEHOLDER, ProcessSpec, Protocol, TemporaryArtifact, TerminalMode,
    Transport,
};
use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, PlanContext, PlanError, SecretRef, coding_model_profile,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
pub(crate) const PROVIDER_URL_ENVIRONMENT: &str = "NAN_HARNESS_PROVIDER_BASE_URL";

pub(crate) struct DirectLaunch {
    pub(crate) arguments: Vec<String>,
    pub(crate) credential_target: &'static str,
    pub(crate) public_environment: BTreeMap<String, String>,
    pub(crate) removed_environment: BTreeSet<String>,
    pub(crate) temporary_artifacts: Vec<TemporaryArtifact>,
    pub(crate) configuration_overlays: Vec<ConfigurationOverlay>,
}

pub(crate) fn build_direct_plan(
    context: &PlanContext,
    launch: DirectLaunch,
) -> Result<LaunchPlan, PlanError> {
    let credential_ref =
        SecretRef::new(PROVIDER_CREDENTIAL_REFERENCE).map_err(|error| PlanError::InvalidField {
            field: "transport",
            message: error.to_string(),
        })?;
    let credential_target = launch.credential_target.to_owned();
    let mut redacted = BTreeSet::from([credential_target.clone(), "NAN_API_KEY".to_owned()]);
    redacted.extend(launch.removed_environment.iter().cloned());

    Ok(LaunchPlan {
        schema_version: 1,
        launch_id: context.launch_id.clone(),
        harness: context.harness.clone(),
        model: context.model.clone(),
        transport: Transport::DirectChat {
            protocol: Protocol::ChatCompletions,
            base_url: PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
            credential_target: credential_target.clone(),
        },
        process: ProcessSpec {
            arguments: launch.arguments,
            working_directory: context.working_directory.clone(),
            terminal: TerminalMode::Inherit,
            forward_signals: true,
            preserve_exit_code: true,
        },
        environment: EnvironmentOverlay {
            public: launch.public_environment,
            secrets: BTreeMap::from([(credential_target, credential_ref)]),
            remove: launch.removed_environment,
        },
        temporary_artifacts: launch.temporary_artifacts,
        configuration_overlays: launch.configuration_overlays,
        cleanup: CleanupPolicy {
            terminate_bridge: false,
            delete_temporary_artifacts: true,
            grace_period_ms: 3_000,
        },
        observability: ObservabilityPolicy {
            format: context.observability_format,
            payload_capture: false,
            redact_environment_names: redacted,
        },
    })
}

pub(crate) fn provider_environment() -> BTreeMap<String, String> {
    BTreeMap::from([(
        PROVIDER_URL_ENVIRONMENT.to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    )])
}

pub(crate) fn validate_routing_arguments(
    arguments: &[String],
    reserved: &[&str],
) -> Result<(), PlanError> {
    if let Some(argument) = arguments.iter().find(|argument| {
        reserved.iter().any(|reserved| {
            argument.as_str() == *reserved
                || argument
                    .strip_prefix(reserved)
                    .is_some_and(|suffix| suffix.starts_with('='))
        })
    }) {
        return Err(PlanError::InvalidField {
            field: "process.arguments",
            message: format!("argument '{argument}' conflicts with NaN Harness routing"),
        });
    }
    Ok(())
}

pub struct ModelDescription {
    pub display_name: String,
    pub description: String,
    pub context_window: u64,
    pub max_tokens: u64,
    pub image_input: bool,
    pub reasoning: ReasoningPolicy,
}

#[must_use]
pub fn describe_model(model_id: &str) -> ModelDescription {
    let model =
        coding_model_profile(model_id).unwrap_or_else(|| CodingModelProfile::generic(model_id));
    ModelDescription {
        display_name: model.display_name,
        description: model.description,
        context_window: model.context_window,
        max_tokens: model.max_output_tokens,
        image_input: model.image_input,
        reasoning: model.reasoning,
    }
}
