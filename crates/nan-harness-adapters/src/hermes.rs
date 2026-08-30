use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, ConfigurationOverlay,
    HERMES_MODEL_CATALOG_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "NAN_API_KEY";
const CONFIG_OVERLAY_ID: &str = "hermes-home";
const CONFIG_PATH: &str = "{artifact:hermes-home}";

fn model_provider_files() -> Vec<OverlayFile> {
    vec![
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
    ]
}

fn search_provider_files() -> Vec<OverlayFile> {
    vec![
        OverlayFile {
            path: "plugins/web/nan_harness/__init__.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "from .provider import NanHarnessWebSearchProvider\n\n\ndef register(ctx):\n    ctx.register_web_search_provider(NanHarnessWebSearchProvider())\n"
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/web/nan_harness/provider.py".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                r#"import os

import httpx

from agent.web_search_provider import WebSearchProvider


class NanHarnessWebSearchProvider(WebSearchProvider):
    @property
    def name(self):
        return "nan-harness"

    @property
    def display_name(self):
        return "nan-search"

    def is_available(self):
        return bool(os.getenv("NAN_API_KEY", "").strip())

    def search(self, query, limit=5):
        try:
            response = httpx.post(
                "{BRIDGE_BASE_URL_PLACEHOLDER}/v1/search",
                headers={{"Authorization": f"Bearer {{os.environ['NAN_API_KEY']}}"}},
                json={{"query": query, "maxResults": min(max(int(limit), 1), 20)}},
                timeout=60,
            )
            response.raise_for_status()
            results = response.json().get("results", [])
            return {{
                "success": True,
                "data": {{
                    "web": [
                        {{
                            "title": item.get("title", ""),
                            "url": item.get("url", ""),
                            "description": item.get("snippet", ""),
                            "position": position,
                        }}
                        for position, item in enumerate(results, start=1)
                    ]
                }},
            }}
        except Exception:
            return {{"success": False, "error": "NH-SEARCH-HTTP"}}
"#
            ),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "plugins/web/nan_harness/plugin.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "name: nan-search\nkind: backend\nversion: 1.0.0\ndescription: nan-search\nauthor: NaN\nprovides_web_providers:\n  - nan-harness\n"
                .to_owned(),
            policy: OverlayFilePolicy::Replace,
        },
        OverlayFile {
            path: "config.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: format!(
                "{{{NAN_SEARCH_BLOCK_BEGIN}\"plugins\": {{\"enabled\": [\"web/nan_harness\"]}}, \"web\": {{\"search_backend\": \"nan-harness\"}}{NAN_SEARCH_BLOCK_END}}}\n"
            ),
            policy: OverlayFilePolicy::MergeYaml,
        },
    ]
}

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
                    files: model_provider_files()
                        .into_iter()
                        .chain(search_provider_files())
                        .collect(),
                    lifecycle: ArtifactLifecycle::Launch,
                }],
            },
        )
    }
}
