use crate::error::PlanError;
use crate::harness::{DetectedHarness, HarnessKind};
use crate::launch_plan::{
    ContextLimit, LaunchId, LaunchPlan, LaunchPlanValidator, ObservabilityFormat, WebSearchPolicy,
};
use crate::model::ResolvedModel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanContext {
    pub launch_id: LaunchId,
    pub harness: DetectedHarness,
    pub model: ResolvedModel,
    pub working_directory: String,
    pub user_arguments: Vec<String>,
    pub web_search_policy: WebSearchPolicy,
    pub observability_format: ObservabilityFormat,
    pub session_max_tokens: Option<u64>,
    pub context_limit: Option<ContextLimit>,
}

pub trait HarnessAdapter {
    fn kind(&self) -> HarnessKind;

    /// Builds a data-only launch plan for this adapter.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] when the context cannot produce a valid plan.
    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError>;
}

/// Builds a plan and checks every cross-field invariant.
///
/// # Errors
///
/// Returns [`PlanError`] when the adapter does not match the requested harness,
/// planning fails, or the produced plan violates the contract.
pub fn build_validated_plan(
    adapter: &dyn HarnessAdapter,
    context: &PlanContext,
) -> Result<LaunchPlan, PlanError> {
    if adapter.kind() != context.harness.kind {
        return Err(PlanError::AdapterMismatch {
            adapter: adapter.kind(),
            requested: context.harness.kind,
        });
    }
    let mut plan = adapter.plan(context)?;
    // Limits are launch metadata owned by the common planner. Keeping this
    // assignment here makes every adapter opt in consistently without allowing
    // a harness-specific adapter to silently drop a requested budget.
    plan.session_max_tokens = context.session_max_tokens;
    plan.context_limit.clone_from(&context.context_limit);
    LaunchPlanValidator::validate(&plan)?;
    Ok(plan)
}
