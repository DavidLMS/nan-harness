use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER, CleanupPolicy,
    ConfigurationOverlay, EnvironmentOverlay, ListenAddress, ObservabilityPolicy, OverlayFile,
    OverlayFilePolicy, ProcessSpec, Protocol, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode, TerminalMode, Transport, USER_HOME_PLACEHOLDER,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, SecretRef,
};
use std::collections::{BTreeMap, BTreeSet};

const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
const SESSION_TOKEN_REFERENCE: &str = "bridge_session_token";
const SESSION_TOKEN_ENVIRONMENT: &str = "NAN_HARNESS_SESSION_TOKEN";
const MODEL_CATALOG_ARTIFACT: &str = "codex-model-catalog";
const MODEL_CATALOG_PATH_PLACEHOLDER: &str = "{artifact:codex-model-catalog}";
const CODEX_HOME_OVERLAY_ID: &str = "codex-home";
const CODEX_HOME_PATH_PLACEHOLDER: &str = "{artifact:codex-home}";
const CODEX_STATE_FILES: [&str; 3] = ["state_5.sqlite", "state_5.sqlite-wal", "state_5.sqlite-shm"];

#[derive(Debug, Default)]
pub struct CodexAdapter;

impl HarnessAdapter for CodexAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Codex
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_user_arguments(&context.user_arguments)?;
        let provider_credential_ref = secret_ref(PROVIDER_CREDENTIAL_REFERENCE)?;
        let session_token_ref = secret_ref(SESSION_TOKEN_REFERENCE)?;
        let model_config = serde_json::to_string(&context.model.resolved_id).map_err(|error| {
            PlanError::InvalidField {
                field: "configurationOverlays.files.contentTemplate",
                message: format!("could not serialize Codex model configuration: {error}"),
            }
        })?;
        let mut arguments = routing_arguments();
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
                public: BTreeMap::from([(
                    "CODEX_HOME".to_owned(),
                    CODEX_HOME_PATH_PLACEHOLDER.to_owned(),
                )]),
                secrets: BTreeMap::from([(
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                    session_token_ref,
                )]),
                remove: BTreeSet::from([
                    "CODEX_API_KEY".to_owned(),
                    "NAN_API_KEY".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    "CODEX_CI".to_owned(),
                    "CODEX_THREAD_ID".to_owned(),
                ]),
            },
            temporary_artifacts: vec![TemporaryArtifact {
                id: MODEL_CATALOG_ARTIFACT.to_owned(),
                kind: TemporaryArtifactKind::File,
                path_hint: "codex-model-catalog.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: Some(CODEX_MODEL_CATALOG_PLACEHOLDER.to_owned()),
                lifecycle: ArtifactLifecycle::Launch,
            }],
            configuration_overlays: vec![ConfigurationOverlay {
                id: CODEX_HOME_OVERLAY_ID.to_owned(),
                path_hint: "codex-home".to_owned(),
                source_path: format!("{USER_HOME_PLACEHOLDER}/.codex"),
                files: std::iter::once(OverlayFile {
                    path: "config.toml".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: format!("model = {model_config}\n"),
                    policy: OverlayFilePolicy::MergeToml,
                })
                .chain(CODEX_STATE_FILES.iter().map(|path| OverlayFile {
                    path: (*path).to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: String::new(),
                    policy: OverlayFilePolicy::CopyBinary,
                }))
                .collect(),
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
                    "CODEX_API_KEY".to_owned(),
                    "NAN_API_KEY".to_owned(),
                    "OPENAI_API_KEY".to_owned(),
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                ]),
            },
        })
    }
}

fn routing_arguments() -> Vec<String> {
    let provider = format!(
        concat!(
            "model_providers.nan_harness={{",
            "name=\"NaN Harness\",",
            "base_url=\"{}/v1\",",
            "env_key=\"{}\",",
            "wire_api=\"responses\",",
            "request_max_retries=0,",
            "stream_max_retries=0,",
            "supports_websockets=false,",
            "supports_standalone_web_search=true,",
            "requires_openai_auth=false",
            "}}"
        ),
        BRIDGE_BASE_URL_PLACEHOLDER, SESSION_TOKEN_ENVIRONMENT
    );
    vec![
        "-c".to_owned(),
        "model_provider=\"nan_harness\"".to_owned(),
        "-c".to_owned(),
        provider,
        "-c".to_owned(),
        "features.standalone_web_search=true".to_owned(),
        "-c".to_owned(),
        "features.responses_websockets=false".to_owned(),
        "-c".to_owned(),
        "features.responses_websockets_v2=false".to_owned(),
        "-c".to_owned(),
        "suppress_unstable_features_warning=true".to_owned(),
        "--disable".to_owned(),
        "apps".to_owned(),
        "-c".to_owned(),
        "mcp_servers.openaiDeveloperDocs.enabled=false".to_owned(),
        "-c".to_owned(),
        format!("model_catalog_json=\"{MODEL_CATALOG_PATH_PLACEHOLDER}\""),
    ]
}

fn validate_user_arguments(arguments: &[String]) -> Result<(), PlanError> {
    const RESERVED: [&str; 10] = [
        "-c",
        "--config",
        "-m",
        "--model",
        "--oss",
        "--local-provider",
        "-p",
        "--profile",
        "--ignore-user-config",
        "--strict-config",
    ];
    if let Some(argument) = arguments.iter().find(|argument| {
        RESERVED.iter().any(|reserved| {
            argument.as_str() == *reserved
                || argument
                    .strip_prefix(reserved)
                    .is_some_and(|suffix| suffix.starts_with('='))
        })
    }) {
        return Err(PlanError::InvalidField {
            field: "process.arguments",
            message: format!("argument '{argument}' conflicts with NaN Harness routing"),
        });
    }
    Ok(())
}

fn secret_ref(value: &str) -> Result<SecretRef, PlanError> {
    SecretRef::new(value).map_err(|error| PlanError::InvalidField {
        field: "transport",
        message: error.to_string(),
    })
}
