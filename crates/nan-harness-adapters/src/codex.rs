use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, CODEX_HOME_ARTIFACT_PLACEHOLDER,
    CODEX_HOME_OVERLAY_ID, CODEX_HOME_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER,
    CODEX_PROFILE_ARTIFACT_ID, CleanupPolicy, ConfigurationOverlay, EnvironmentOverlay,
    LaunchScopedFile, ListenAddress, ObservabilityPolicy, OverlayFile, OverlayFilePolicy,
    ProcessSpec, Protocol, SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, TemporaryArtifact,
    TemporaryArtifactKind, TemporaryArtifactMode, TerminalMode, Transport,
};
use nan_harness_core::{
    HarnessAdapter, HarnessCapability, HarnessKind, LaunchPlan, PlanContext, PlanError, SecretRef,
};
use std::collections::{BTreeMap, BTreeSet};

const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
const SESSION_TOKEN_REFERENCE: &str = "bridge_session_token";
const SESSION_TOKEN_ENVIRONMENT: &str = "NAN_HARNESS_SESSION_TOKEN";
const MODEL_CATALOG_ARTIFACT: &str = "codex-model-catalog";
const MODEL_CATALOG_PATH_PLACEHOLDER: &str = "{artifact:codex-model-catalog}";
const PROFILE_OWNERSHIP_PREFIX: &str = "nan-harness-launch_";

#[derive(Debug, Default)]
pub struct CodexAdapter;

struct CodexLaunchConfiguration {
    arguments: Vec<String>,
    public_environment: BTreeMap<String, String>,
    overlays: Vec<ConfigurationOverlay>,
    scoped_files: Vec<LaunchScopedFile>,
}

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
        let configuration = launch_configuration(context, &model_config);
        let mut arguments = configuration.arguments;
        arguments.extend(routing_arguments(&model_config));
        arguments.extend(context.user_arguments.iter().cloned());

        Ok(LaunchPlan {
            schema_version: 2,
            launch_id: context.launch_id.clone(),
            harness: context.harness.clone(),
            model: context.model.clone(),
            web_search_policy: context.web_search_policy,
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
                public: configuration.public_environment,
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
            configuration_overlays: configuration.overlays,
            launch_scoped_files: configuration.scoped_files,
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

fn launch_configuration(context: &PlanContext, model_config: &str) -> CodexLaunchConfiguration {
    let profile_content = format!(
        "model = {model_config}\nmodel_reasoning_effort = \"{SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER}\"\n"
    );
    if context
        .harness
        .capabilities
        .contains(&HarnessCapability::CodexConfigProfile)
    {
        let profile_name = format!("nan-harness-{}", context.launch_id);
        return CodexLaunchConfiguration {
            arguments: vec!["--profile".to_owned(), profile_name.clone()],
            public_environment: BTreeMap::new(),
            overlays: Vec::new(),
            scoped_files: vec![LaunchScopedFile {
                id: CODEX_PROFILE_ARTIFACT_ID.to_owned(),
                directory: CODEX_HOME_PLACEHOLDER.to_owned(),
                file_name: format!("{profile_name}.config.toml"),
                ownership_prefix: PROFILE_OWNERSHIP_PREFIX.to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: profile_content,
                lifecycle: ArtifactLifecycle::Launch,
            }],
        };
    }

    CodexLaunchConfiguration {
        arguments: Vec::new(),
        public_environment: BTreeMap::from([(
            "CODEX_HOME".to_owned(),
            CODEX_HOME_ARTIFACT_PLACEHOLDER.to_owned(),
        )]),
        overlays: vec![ConfigurationOverlay {
            id: CODEX_HOME_OVERLAY_ID.to_owned(),
            path_hint: "codex-home".to_owned(),
            source_path: CODEX_HOME_PLACEHOLDER.to_owned(),
            files: vec![OverlayFile {
                path: "config.toml".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: profile_content,
                policy: OverlayFilePolicy::MergeToml,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        }],
        scoped_files: Vec::new(),
    }
}

fn routing_arguments(model_config: &str) -> Vec<String> {
    let provider = format!(
        concat!(
            "model_providers.nan_harness={{",
            "name=\"nan-harness\",",
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
        format!("model={model_config}"),
        "-c".to_owned(),
        format!("model_reasoning_effort=\"{SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER}\""),
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
            message: format!("argument '{argument}' conflicts with nan-harness routing"),
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
