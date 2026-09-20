use crate::direct::{
    DirectLaunch, PROVIDER_CREDENTIAL_REFERENCE, build_direct_plan, provider_environment,
    validate_routing_arguments,
};
use crate::search::nan_search_mcp_overlay;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CLINE_MODEL_CATALOG_PLACEHOLDER, ConfigurationOverlay, OverlayFile,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
use serde_json::json;
use std::collections::BTreeSet;

const CREDENTIAL_TARGET: &str = "OPENAI_API_KEY";
const CONFIG_OVERLAY_ID: &str = "cline-config";
const CONFIG_PATH: &str = "{artifact:cline-config}";
const DATA_DIR_PATH: &str = "{artifact:cline-config}/data";

fn is_flag(arguments: &[String], flag: &str) -> bool {
    arguments.iter().any(|argument| {
        argument == flag
            || argument
                .strip_prefix(flag)
                .is_some_and(|suffix| suffix.starts_with('='))
    })
}

fn provider_settings(model_id: &str) -> Result<String, PlanError> {
    serde_json::to_string(&json!({
        "lastUsedProvider": "openai-compatible",
        "providers": {
            "openai-compatible": {
                "settings": {
                    "apiKey": format!("{{secret:{PROVIDER_CREDENTIAL_REFERENCE}}}"),
                    "baseUrl": PROVIDER_BASE_URL_PLACEHOLDER,
                    "model": model_id,
                    "provider": "openai-compatible"
                },
                "tokenSource": "manual",
                "updatedAt": "1970-01-01T00:00:00.000Z"
            }
        },
        "version": 1
    }))
    .map_err(|error| PlanError::InvalidField {
        field: "configurationOverlays.files.contentTemplate",
        message: DiagnosticText::new(|locale| {
            detail_messages::detail_serialize_cline_settings_failed(locale, &(error))
        }),
    })
}

fn model_catalog() -> Result<String, PlanError> {
    serde_json::to_string(&json!({
        "version": 1,
        "providers": {
            "openai-compatible": {
                "models": CLINE_MODEL_CATALOG_PLACEHOLDER
            }
        }
    }))
    .map_err(|error| PlanError::InvalidField {
        field: "configurationOverlays.files.contentTemplate",
        message: DiagnosticText::new(|locale| {
            detail_messages::detail_serialize_cline_model_catalog_failed(locale, &(error))
        }),
    })
}

fn arguments(context: &PlanContext, model_id: &str) -> Vec<String> {
    let mut arguments = vec![
        "--config".to_owned(),
        CONFIG_PATH.to_owned(),
        "--provider".to_owned(),
        "openai-compatible".to_owned(),
        "--model".to_owned(),
        model_id.to_owned(),
    ];
    // Cline's non-interactive JSON path can attach to its Hub. On Windows 3.0.x,
    // that path reports successful shell tool calls without running the command.
    // An explicit data directory selects Cline's local runtime while preserving
    // act-mode's complete tool inventory.
    if is_flag(&context.user_arguments, "--json") {
        arguments.extend(["--data-dir".to_owned(), DATA_DIR_PATH.to_owned()]);
    }
    arguments.extend(context.user_arguments.iter().cloned());
    arguments
}

fn configuration_overlay(provider_settings: String, model_catalog: String) -> ConfigurationOverlay {
    ConfigurationOverlay {
        id: CONFIG_OVERLAY_ID.to_owned(),
        path_hint: "cline".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.cline"),
        files: vec![
            OverlayFile {
                path: "data/settings/providers.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: provider_settings,
                policy: OverlayFilePolicy::MergeJson,
            },
            OverlayFile {
                path: "data/settings/models.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: model_catalog,
                policy: OverlayFilePolicy::MergeJson,
            },
            OverlayFile {
                path: "data/settings/mcp_settings.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: nan_search_mcp_overlay(CREDENTIAL_TARGET),
                policy: OverlayFilePolicy::MergeJson,
            },
        ],
        lifecycle: ArtifactLifecycle::Launch,
    }
}

#[derive(Debug, Default)]
pub struct ClineAdapter;

impl HarnessAdapter for ClineAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Cline
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &[
                "--config",
                "--data-dir",
                "--provider",
                "-P",
                "--model",
                "-m",
                "--key",
                "-k",
            ],
        )?;
        let model_id = &context.model.resolved_id;

        build_direct_plan(
            context,
            DirectLaunch {
                arguments: arguments(context, model_id),
                credential_target: CREDENTIAL_TARGET,
                public_environment: provider_environment(),
                removed_environment: BTreeSet::from([
                    "CLINE_DEFAULT_MODEL_ID".to_owned(),
                    "CLINE_MODEL".to_owned(),
                    "CLINE_PROVIDER".to_owned(),
                    "OPENAI_BASE_URL".to_owned(),
                ]),
                temporary_artifacts: Vec::new(),
                configuration_overlays: vec![configuration_overlay(
                    provider_settings(model_id)?,
                    model_catalog()?,
                )],
            },
        )
    }
}
