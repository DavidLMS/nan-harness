use crate::direct::validate_routing_arguments;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, CleanupPolicy, ConfigurationOverlay,
    EnvironmentOverlay, ListenAddress, ObservabilityPolicy, OverlayFile, OverlayFilePolicy,
    ProcessSpec, Protocol, TemporaryArtifactMode, TerminalMode, Transport, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, SecretRef,
};
use std::collections::{BTreeMap, BTreeSet};

const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
const SESSION_TOKEN_REFERENCE: &str = "bridge_session_token";
const SESSION_TOKEN_ENVIRONMENT: &str = "OPENAI_API_KEY";
const HOME_OVERLAY: &str = "roo-home";
const HOME_OVERLAY_PATH: &str = "{artifact:roo-home}";

#[derive(Debug, Default)]
pub struct RooCodeAdapter;

impl HarnessAdapter for RooCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::RooCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_routing_arguments(
            &context.user_arguments,
            &["--provider", "--model", "-m", "--api-key", "-k"],
        )?;
        let provider_credential_ref = secret_ref(PROVIDER_CREDENTIAL_REFERENCE)?;
        let session_token_ref = secret_ref(SESSION_TOKEN_REFERENCE)?;
        let mut arguments = vec![
            "--provider".to_owned(),
            "openai-native".to_owned(),
            "--model".to_owned(),
            context.model.resolved_id.clone(),
            "--reasoning-effort".to_owned(),
            "disabled".to_owned(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        Ok(LaunchPlan {
            schema_version: 1,
            launch_id: context.launch_id.clone(),
            harness: context.harness.clone(),
            model: context.model.clone(),
            transport: Transport::ResponsesBridge {
                client_protocol: Protocol::OpenAiResponses,
                upstream_protocol: Protocol::ChatCompletions,
                listen: ListenAddress {
                    host: "127.0.0.1".to_owned(),
                    port: 0,
                },
                provider_credential_ref,
                session_token_ref: session_token_ref.clone(),
            },
            process: ProcessSpec {
                arguments,
                working_directory: context.working_directory.clone(),
                terminal: TerminalMode::Inherit,
                forward_signals: true,
                preserve_exit_code: true,
            },
            environment: EnvironmentOverlay {
                public: BTreeMap::from([
                    ("HOME".to_owned(), HOME_OVERLAY_PATH.to_owned()),
                    (
                        "OPENAI_BASE_URL".to_owned(),
                        BRIDGE_BASE_URL_PLACEHOLDER.to_owned(),
                    ),
                ]),
                secrets: BTreeMap::from([(
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                    session_token_ref,
                )]),
                remove: BTreeSet::from([
                    "NAN_API_KEY".to_owned(),
                    "OPENAI_API_BASE".to_owned(),
                    "OPENAI_HOST".to_owned(),
                    "OPENAI_ORG_ID".to_owned(),
                    "OPENAI_PROJECT_ID".to_owned(),
                ]),
            },
            temporary_artifacts: Vec::new(),
            configuration_overlays: vec![ConfigurationOverlay {
                id: HOME_OVERLAY.to_owned(),
                path_hint: HOME_OVERLAY.to_owned(),
                source_path: USER_HOME_PLACEHOLDER.to_owned(),
                files: vec![
                    OverlayFile {
                        path: ".vscode-mock/global-storage/global-state.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: format!(
                            r#"{{"openAiNativeBaseUrl":"{BRIDGE_BASE_URL_PLACEHOLDER}"}}"#
                        ),
                        policy: OverlayFilePolicy::MergeJson,
                    },
                    OverlayFile {
                        path: ".vscode-mock/global-storage/secrets.json".to_owned(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: "{}".to_owned(),
                        policy: OverlayFilePolicy::Copy,
                    },
                ],
                lifecycle: ArtifactLifecycle::Launch,
            }],
            cleanup: CleanupPolicy {
                terminate_bridge: true,
                delete_temporary_artifacts: true,
                grace_period_ms: 3_000,
            },
            observability: ObservabilityPolicy {
                format: context.observability_format,
                payload_capture: false,
                redact_environment_names: BTreeSet::from([
                    "NAN_API_KEY".to_owned(),
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                ]),
            },
        })
    }
}

fn secret_ref(value: &str) -> Result<SecretRef, PlanError> {
    SecretRef::new(value).map_err(|error| PlanError::InvalidField {
        field: "transport",
        message: error.to_string(),
    })
}
