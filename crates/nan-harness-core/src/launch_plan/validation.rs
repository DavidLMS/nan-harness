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
        return invalid("schemaVersion", "only schema version 2 is supported");
    }
    if plan.harness.executable.is_empty() {
        return invalid("harness.executable", "cannot be empty");
    }
    if plan.harness.detected_version.is_empty() {
        return invalid("harness.detectedVersion", "cannot be empty");
    }
    if plan.model.requested_id.is_empty() || plan.model.resolved_id.is_empty() {
        return invalid("model", "requested and resolved IDs cannot be empty");
    }
    if !Path::new(&plan.process.working_directory).is_absolute() {
        return invalid("process.workingDirectory", "must be an absolute path");
    }
    Ok(())
}

fn validate_cleanup(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.cleanup.grace_period_ms > 30_000 {
        return invalid("cleanup.gracePeriodMs", "cannot exceed 30000");
    }
    if plan.transport.is_bridge() != plan.cleanup.terminate_bridge {
        return invalid(
            "cleanup.terminateBridge",
            "must be true exactly when the selected transport uses a bridge",
        );
    }
    if (!plan.temporary_artifacts.is_empty()
        || !plan.configuration_overlays.is_empty()
        || !plan.launch_scoped_files.is_empty())
        && !plan.cleanup.delete_temporary_artifacts
    {
        return invalid(
            "cleanup.deleteTemporaryArtifacts",
            "must be true when the plan creates temporary artifacts",
        );
    }
    Ok(())
}

fn validate_observability(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.observability.payload_capture {
        invalid(
            "observability.payloadCapture",
            "payload capture is forbidden in schema version 1",
        )
    } else {
        Ok(())
    }
}

pub(super) fn invalid(field: &'static str, message: impl Into<String>) -> Result<(), PlanError> {
    Err(PlanError::InvalidField {
        field,
        message: message.into(),
    })
}

pub(super) fn unsafe_resource(
    resource_id: &str,
    reason: impl Into<String>,
) -> Result<(), PlanError> {
    Err(PlanError::UnsafeTemporaryArtifact {
        artifact_id: resource_id.to_owned(),
        reason: reason.into(),
    })
}
