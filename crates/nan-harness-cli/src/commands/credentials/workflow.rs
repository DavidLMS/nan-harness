mod dispatch;
mod management;
mod onboarding;
mod prompt;
mod recovery;

use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::ResolvedConfig;

pub(crate) use dispatch::run;
pub(crate) use onboarding::{
    resolve_existing_config, resolve_or_onboard, resolve_saved_or_onboard,
    saved_credential_fingerprint,
};

#[derive(Debug)]
pub(crate) struct ResolvedLaunchConfig {
    pub(crate) config: ResolvedConfig,
    pub(crate) model_catalog: Option<Vec<CodingModelProfile>>,
}

#[cfg(test)]
pub(super) use onboarding::{
    render_first_harness_hint, render_missing_credential_hint, resolve_or_onboard_with,
};
