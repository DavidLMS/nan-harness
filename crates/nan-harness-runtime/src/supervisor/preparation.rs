use crate::prepared::{BridgePreparation, PreparedError, PreparedLaunch};
use nan_harness_core::{CodingModelProfile, LaunchPlan};
use std::path::PathBuf;

pub(super) struct PreparedHarnessLaunch {
    pub(super) prepared: PreparedLaunch,
    pub(super) temporary_root: Option<PathBuf>,
}

impl PreparedHarnessLaunch {
    pub(super) fn prepare(
        plan: &LaunchPlan,
        provider_base_url: &str,
        bridge: Option<BridgePreparation>,
        model_catalog: Option<&[CodingModelProfile]>,
    ) -> Result<Self, PreparedError> {
        let prepared = PreparedLaunch::prepare(plan, provider_base_url, bridge, model_catalog)?;
        let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
        Ok(Self {
            prepared,
            temporary_root,
        })
    }
}

fn has_temporary_resources(plan: &LaunchPlan) -> bool {
    !plan.temporary_artifacts.is_empty()
        || !plan.configuration_overlays.is_empty()
        || !plan.launch_scoped_files.is_empty()
}
