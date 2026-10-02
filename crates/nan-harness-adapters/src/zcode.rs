use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use crate::search::{NAN_SEARCH_MCP_ID, nan_search_mcp_server};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, TemporaryArtifact,
    TemporaryArtifactKind, TemporaryArtifactMode, ZCODE_PROVIDER_CONFIG_PLACEHOLDER,
};
use nan_harness_core::{
    HarnessAdapter, HarnessCapability, HarnessKind, LaunchPlan, PlanContext, PlanError,
    WebSearchPolicy,
};
use nan_harness_i18n::DiagnosticText;
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "ZCODE_NAN_API_KEY";

#[derive(Debug, Default)]
pub struct ZCodeAdapter;

impl HarnessAdapter for ZCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::ZCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--model",
                "-m",
                "--provider",
                "--config",
                "--nanh-source-info",
            ],
        )?;
        let private_config = context
            .harness
            .capabilities
            .contains(&HarnessCapability::ZCodeConfigOverride);
        if !private_config && context.web_search_policy != WebSearchPolicy::Disabled {
            return Err(PlanError::InvalidField {
                field: "harness.capabilities",
                message: DiagnosticText::new(
                    nan_harness_i18n::messages::detail_zcode_private_configuration_required,
                ),
            });
        }
        let mut public_environment = provider_environment();
        public_environment.insert(
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE".to_owned(),
            "{artifact:zcode-provider}".to_owned(),
        );
        public_environment.insert(
            "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE".to_owned(),
            "{artifact:zcode-builtin}".to_owned(),
        );
        // The registry contains only live NaN models. Foreign session pins fail closed.
        let mut temporary_artifacts = vec![
            artifact("zcode-provider", ZCODE_PROVIDER_CONFIG_PLACEHOLDER.to_owned()),
            artifact("zcode-builtin", json!({"schemaVersion": 1, "revision": 0, "config": {
                "providerConfigRules": {"templateRules": [], "providerRules": []},
                "modelConfigRules": {"modelRules": [], "modelApiRules": [], "providerSiteRules": [], "templateModelRules": [], "builtinProviderModelRules": []}
            }}).to_string()),
        ];
        if private_config {
            let mut server = nan_search_mcp_server(CREDENTIAL_TARGET);
            server["type"] = json!("stdio");
            let search =
                json!({"features": {"mcp": true}, "mcp": {"servers": {NAN_SEARCH_MCP_ID: server}}});
            let search = search.to_string();
            temporary_artifacts.push(artifact(
                "zcode-project",
                format!(
                    "{{{NAN_SEARCH_BLOCK_BEGIN}{}{NAN_SEARCH_BLOCK_END}}}",
                    &search[1..search.len() - 1]
                ),
            ));
            public_environment.insert(
                "NAN_HARNESS_ZCODE_PROJECT_CONFIG_FILE".to_owned(),
                "{artifact:zcode-project}".to_owned(),
            );
        }
        build_direct_plan(
            context,
            DirectLaunch {
                arguments: context.user_arguments.clone(),
                credential_target: CREDENTIAL_TARGET,
                public_environment,
                removed_environment: BTreeSet::from([
                    "NAN_API_KEY".to_owned(),
                    "ZCODE_BUILTIN_PROVIDER_BUNDLED_CONFIG_FILE".to_owned(),
                ]),
                temporary_artifacts,
                configuration_overlays: Vec::new(),
            },
        )
    }
}

fn artifact(id: &str, contents: String) -> TemporaryArtifact {
    TemporaryArtifact {
        id: id.to_owned(),
        kind: TemporaryArtifactKind::File,
        path_hint: format!("{id}.json"),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: Some(contents),
        lifecycle: ArtifactLifecycle::Launch,
    }
}
