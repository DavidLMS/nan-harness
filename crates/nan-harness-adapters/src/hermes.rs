use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, HERMES_MODEL_CATALOG_PLACEHOLDER, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "hermes-home";
const CONFIG_PATH: &str = "{artifact:hermes-home}";

#[derive(Debug, Default)]
pub struct HermesAdapter;

impl HarnessAdapter for HermesAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Hermes
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(&context.user_arguments, &["--model", "-m", "--provider"])?;
        let mut public_environment = provider_environment();
        public_environment.insert("HERMES_HOME".to_owned(), CONFIG_PATH.to_owned());
        let mut arguments = vec![
            "--provider".to_owned(),
            "nan".to_owned(),
            "--model".to_owned(),
            context.model.resolved_id.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        build_direct_plan(
            context,
            DirectLaunch {
                arguments,
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "CUSTOM_BASE_URL".to_owned(),
                    "HERMES_INFERENCE_MODEL".to_owned(),
                    "HERMES_INFERENCE_PROVIDER".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![ConfigurationOverlay {
                    id: CONFIG_OVERLAY_ID.to_owned(),
                    path_hint: "hermes".to_owned(),
                    source_path: format!("{USER_HOME_PLACEHOLDER}/.hermes"),
                    files: vec![
                        OverlayFile {
                            path: "plugins/model-providers/nan/__init__.py".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: format!(
                                r#"from providers import register_provider
from providers.base import ProviderProfile


class NanProviderProfile(ProviderProfile):
    def fetch_models(self, **_kwargs):
        return list(self.fallback_models)


nan = NanProviderProfile(
    name="nan",
    display_name="NaN",
    description="NaN model access",
    env_vars=("NAN_API_KEY",),
    base_url="{PROVIDER_BASE_URL_PLACEHOLDER}",
    auth_type="api_key",
    fallback_models=tuple({HERMES_MODEL_CATALOG_PLACEHOLDER}),
)

register_provider(nan)
"#
                            ),
                            policy: OverlayFilePolicy::Replace,
                        },
                        OverlayFile {
                            path: "plugins/model-providers/nan/plugin.yaml".to_owned(),
                            mode: TemporaryArtifactMode::OwnerFile,
                            content_template: "name: nan-provider\nkind: model-provider\nversion: 1.0.0\ndescription: NaN model access\nauthor: NaN\n".to_owned(),
                            policy: OverlayFilePolicy::Replace,
                        },
                    ],
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
