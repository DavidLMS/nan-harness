use crate::pi::{PiAdapter, provider_extension};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};

/// Gentle Shell owns the Pi home and packages; nanh owns only launch-local NaN routing.
pub struct GentleShellAdapter {
    pub preserve_provider: bool,
    pub agent_home: String,
}

impl HarnessAdapter for GentleShellAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        if context.context_limit.is_some() {
            return Err(PlanError::InvalidField {
                field: "context",
                message: nan_harness_i18n::DiagnosticText::new(
                    nan_harness_i18n::messages::detail_gentle_shell_context,
                ),
            });
        }
        let mut plan = PiAdapter.plan(context)?;
        plan.environment.public.insert(
            "NAN_HARNESS_GENTLE_SHELL_HOME".into(),
            self.agent_home.clone(),
        );
        // Keep other providers selectable, particularly in Gentle Shell agent profiles.
        plan.process.arguments.drain(6..8);
        let extension = provider_extension(context.web_search_policy, true)
            .replace("pi.registerProvider(\"nan\", {", "registerGentleNan(pi, {")
            .replace(
                "registerNanReasoning(pi, profiles, true);",
                "if (!preserveGentleProvider) registerNanReasoning(pi, profiles, true);",
            );
        let extension = format!(
            "{}\nconst preserveGentleProvider = {};\n{}",
            include_str!("pi/gentle_shell.js"),
            self.preserve_provider,
            extension
        );
        plan.temporary_artifacts[0].content_template = Some(extension);
        Ok(plan)
    }
}
