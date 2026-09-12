use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
mod environment;
mod placeholders;
mod resources;
mod transport;

use super::LaunchPlan;
use crate::error::PlanError;
use std::path::Path;

pub(super) fn validate(plan: &LaunchPlan) -> Result<(), PlanError> {
    validate_required_fields(plan)?;
    transport::validate(plan)?;
    environment::validate(plan)?;
    resources::validate_artifacts(plan)?;
    resources::validate_configuration_overlays(plan)?;
    resources::validate_launch_scoped_files(plan)?;
    validate_cleanup(plan)?;
    validate_observability(plan)
}

fn validate_required_fields(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.schema_version != 2 {
        return invalid(
            "schemaVersion",
            DiagnosticText::new(detail_messages::detail_only_schema_version_2_is_supported),
        );
    }
    if plan.harness.executable.is_empty() {
        return invalid(
            "harness.executable",
            DiagnosticText::new(detail_messages::detail_cannot_be_empty),
        );
    }
    if plan.harness.detected_version.is_empty() {
        return invalid(
            "harness.detectedVersion",
            DiagnosticText::new(detail_messages::detail_cannot_be_empty),
        );
    }
    if plan.model.requested_id.is_empty() || plan.model.resolved_id.is_empty() {
        return invalid(
            "model",
            DiagnosticText::new(detail_messages::detail_requested_and_resolved_ids_cannot_be_empty),
        );
    }
    if !Path::new(&plan.process.working_directory).is_absolute() {
        return invalid(
            "process.workingDirectory",
            DiagnosticText::new(detail_messages::detail_must_be_an_absolute_path),
        );
    }
    if plan.session_max_tokens.is_some_and(|tokens| tokens == 0) {
        return invalid(
            "sessionMaxTokens",
            DiagnosticText::new(detail_messages::detail_must_be_a_positive_token_count),
        );
    }
    if let Some(context) = &plan.context_limit {
        if context.requested_tokens == 0 {
            return invalid(
                "context.requestedTokens",
                DiagnosticText::new(detail_messages::detail_must_be_a_positive_token_count),
            );
        }
        if context.requested_tokens >= context.effective_context_window {
            return invalid(
                "context.effectiveContextWindow",
                DiagnosticText::new(detail_messages::detail_must_be_greater_than_requestedtokens),
            );
        }
    }
    Ok(())
}

fn validate_cleanup(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.cleanup.grace_period_ms > 30_000 {
        return invalid(
            "cleanup.gracePeriodMs",
            DiagnosticText::new(detail_messages::detail_cannot_exceed_30000),
        );
    }
    if plan.transport.is_bridge() != plan.cleanup.terminate_bridge {
        return invalid(
            "cleanup.terminateBridge",
            DiagnosticText::new(detail_messages::detail_must_be_true_exactly_when_the_selected_transport_uses_a_bridge),
        );
    }
    if (!plan.temporary_artifacts.is_empty()
        || !plan.configuration_overlays.is_empty()
        || !plan.launch_scoped_files.is_empty())
        && !plan.cleanup.delete_temporary_artifacts
    {
        return invalid(
            "cleanup.deleteTemporaryArtifacts",
            DiagnosticText::new(
                detail_messages::detail_must_be_true_when_the_plan_creates_temporary_artifacts,
            ),
        );
    }
    Ok(())
}

fn validate_observability(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.observability.payload_capture {
        invalid(
            "observability.payloadCapture",
            DiagnosticText::new(
                detail_messages::detail_payload_capture_is_forbidden_in_schema_version_1,
            ),
        )
    } else {
        Ok(())
    }
}

pub(super) fn invalid(
    field: &'static str,
    message: impl Into<DiagnosticText>,
) -> Result<(), PlanError> {
    Err(PlanError::InvalidField {
        field,
        message: message.into(),
    })
}

pub(super) fn unsafe_resource(
    resource_id: &str,
    reason: impl Into<DiagnosticText>,
) -> Result<(), PlanError> {
    Err(PlanError::UnsafeTemporaryArtifact {
        artifact_id: resource_id.to_owned(),
        reason: reason.into(),
    })
}
