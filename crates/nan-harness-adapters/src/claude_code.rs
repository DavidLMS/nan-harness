use nan_harness_core::launch_plan::{
    ArtifactLifecycle, BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, CleanupPolicy, EnvironmentOverlay, ListenAddress,
    ObservabilityPolicy, ProcessSpec, Protocol, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode, TerminalMode, Transport,
};
use nan_harness_core::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID, HarnessAdapter,
    HarnessKind, LaunchPlan, PlanContext, PlanError, SecretRef, VersionStatus,
    claude_gateway_model_id,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const SETTINGS_ARTIFACT_ID: &str = "claude-settings";
const SETTINGS_PATH_PLACEHOLDER: &str = "{artifact:claude-settings}";
const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
const SESSION_TOKEN_REFERENCE: &str = "bridge_session_token";

#[derive(Debug, Default)]
pub struct ClaudeCodeAdapter;

impl HarnessAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::ClaudeCode
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        let version_supports_native_auto_mode = version_supports_native_auto_mode(context);
        let native_auto_mode_enabled = version_supports_native_auto_mode
            && context.model.resolved_id == CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID;
        validate_user_arguments(
            &context.user_arguments,
            version_supports_native_auto_mode,
            &context.harness.detected_version,
            &context.model.resolved_id,
        )?;

        let provider_credential_ref = secret_ref(PROVIDER_CREDENTIAL_REFERENCE)?;
        let session_token_ref = secret_ref(SESSION_TOKEN_REFERENCE)?;
        let provider_model_id = &context.model.resolved_id;
        let model = claude_code_model_id(provider_model_id, native_auto_mode_enabled);
        let settings = settings_template(&model)?;
        let mut arguments = vec![
            "--settings".to_owned(),
            SETTINGS_PATH_PLACEHOLDER.to_owned(),
            "--model".to_owned(),
            model.clone(),
        ];
        arguments.extend(context.user_arguments.iter().cloned());

        Ok(LaunchPlan {
            schema_version: 1,
            launch_id: context.launch_id.clone(),
            harness: context.harness.clone(),
            model: context.model.clone(),
            transport: Transport::AnthropicBridge {
                client_protocol: Protocol::AnthropicMessages,
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
                public: public_environment(&model),
                secrets: BTreeMap::from([("ANTHROPIC_AUTH_TOKEN".to_owned(), session_token_ref)]),
                remove: removed_environment(),
            },
            temporary_artifacts: vec![TemporaryArtifact {
                id: SETTINGS_ARTIFACT_ID.to_owned(),
                kind: TemporaryArtifactKind::File,
                path_hint: "claude-settings.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: Some(settings),
                lifecycle: ArtifactLifecycle::Launch,
            }],
            configuration_overlays: Vec::new(),
            launch_scoped_files: Vec::new(),
            cleanup: CleanupPolicy {
                terminate_bridge: true,
                delete_temporary_artifacts: true,
                grace_period_ms: 3_000,
            },
            observability: ObservabilityPolicy {
                format: context.observability_format,
                payload_capture: false,
                redact_environment_names: BTreeSet::from([
                    "ANTHROPIC_AUTH_TOKEN".to_owned(),
                    "NAN_API_KEY".to_owned(),
                ]),
            },
        })
    }
}

fn settings_template(model: &str) -> Result<String, PlanError> {
    let mut environment = public_environment(model);
    environment.insert(
        "ANTHROPIC_AUTH_TOKEN".to_owned(),
        format!("{{secret:{SESSION_TOKEN_REFERENCE}}}"),
    );
    environment.insert("DISABLE_LOGIN_COMMAND".to_owned(), "1".to_owned());
    environment.insert("DISABLE_LOGOUT_COMMAND".to_owned(), "1".to_owned());
    environment.insert(
        CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER.to_owned(),
        String::new(),
    );

    serde_json::to_string(&json!({
        "availableModels": CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
        "model": model,
        "env": environment
    }))
    .map_err(|error| PlanError::InvalidField {
        field: "temporaryArtifacts.contentTemplate",
        message: format!("could not serialize Claude Code settings: {error}"),
    })
}

fn public_environment(model: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "ANTHROPIC_BASE_URL".to_owned(),
            BRIDGE_BASE_URL_PLACEHOLDER.to_owned(),
        ),
        ("ANTHROPIC_MODEL".to_owned(), model.to_owned()),
        ("CLAUDE_CODE_ATTRIBUTION_HEADER".to_owned(), "0".to_owned()),
        (
            "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS".to_owned(),
            "1".to_owned(),
        ),
        (
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".to_owned(),
            "1".to_owned(),
        ),
        (
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS".to_owned(),
            "262144".to_owned(),
        ),
    ])
}

fn removed_environment() -> BTreeSet<String> {
    BTreeSet::from([
        "ANTHROPIC_API_KEY".to_owned(),
        "ANTHROPIC_AWS_API_KEY".to_owned(),
        "ANTHROPIC_AWS_BASE_URL".to_owned(),
        "ANTHROPIC_BEDROCK_BASE_URL".to_owned(),
        "ANTHROPIC_CUSTOM_HEADERS".to_owned(),
        "ANTHROPIC_FOUNDRY_API_KEY".to_owned(),
        "ANTHROPIC_FOUNDRY_BASE_URL".to_owned(),
        "ANTHROPIC_VERTEX_BASE_URL".to_owned(),
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_owned(),
        "CLAUDE_CODE_ENABLE_AUTO_MODE".to_owned(),
        "CLAUDE_CODE_SUBPROCESS_ENV_SCRUB".to_owned(),
        "CLAUDE_CODE_USE_BEDROCK".to_owned(),
        "CLAUDE_CODE_USE_FOUNDRY".to_owned(),
        "CLAUDE_CODE_USE_MANTLE".to_owned(),
        "CLAUDE_CODE_USE_VERTEX".to_owned(),
        "NAN_API_KEY".to_owned(),
    ])
}

fn claude_code_model_id(provider_model_id: &str, supports_native_auto_mode: bool) -> String {
    if supports_native_auto_mode && provider_model_id == CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID {
        CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS.to_owned()
    } else {
        claude_gateway_model_id(provider_model_id)
    }
}

fn version_supports_native_auto_mode(context: &PlanContext) -> bool {
    matches!(
        context.harness.version_status,
        VersionStatus::Tested | VersionStatus::Supported | VersionStatus::NewerUntested
    )
}

fn validate_user_arguments(
    arguments: &[String],
    supports_native_auto_mode: bool,
    detected_version: &str,
    provider_model_id: &str,
) -> Result<(), PlanError> {
    for (index, argument) in arguments.iter().enumerate() {
        let requests_auto_mode = argument == "--enable-auto-mode"
            || argument == "--permission-mode=auto"
            || (argument == "--permission-mode"
                && arguments
                    .get(index + 1)
                    .is_some_and(|value| value == "auto"));
        if requests_auto_mode {
            if !supports_native_auto_mode {
                return Err(PlanError::InvalidField {
                    field: "process.arguments",
                    message: format!(
                        "Claude Code Auto mode requires a supported, parseable Claude Code version; detected '{detected_version}'"
                    ),
                });
            }
            if provider_model_id != CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID {
                return Err(PlanError::InvalidField {
                    field: "process.arguments",
                    message: format!(
                        "Claude Code Auto mode requires the {CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID} model"
                    ),
                });
            }
        }

        let reserved = matches!(
            argument.as_str(),
            "--model"
                | "--settings"
                | "--setting-sources"
                | "--fallback-model"
                | "--teleport"
                | "--cloud"
                | "--remote-control"
                | "--background"
                | "--bg"
        ) || argument.starts_with("--model=")
            || argument.starts_with("--settings=")
            || argument.starts_with("--setting-sources=")
            || argument.starts_with("--fallback-model=");

        if reserved {
            return Err(PlanError::InvalidField {
                field: "process.arguments",
                message: format!(
                    "Claude Code argument '{argument}' conflicts with nan-harness routing"
                ),
            });
        }
    }
    Ok(())
}

fn secret_ref(value: &str) -> Result<SecretRef, PlanError> {
    SecretRef::new(value).map_err(|error| PlanError::InvalidField {
        field: "transport",
        message: error.to_string(),
    })
}
