use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, CleanupPolicy, EnvironmentOverlay,
    FX_GATEWAY_CHAT_URL_PLACEHOLDER, ListenAddress, ObservabilityPolicy, ProcessSpec, TerminalMode,
    Transport,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, PlanError, SecretRef,
};
use std::collections::{BTreeMap, BTreeSet};

const PROVIDER_CREDENTIAL_REFERENCE: &str = "nan_api_key";
const SESSION_TOKEN_REFERENCE: &str = "fx_gateway_session_token";
const SESSION_TOKEN_ENVIRONMENT: &str = "AI_GATEWAY_API_KEY";

#[derive(Debug, Default)]
pub struct FxAdapter;

impl HarnessAdapter for FxAdapter {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Fx
    }

    fn plan(&self, context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        validate_user_arguments(&context.user_arguments)?;
        let provider_credential_ref = secret_ref(PROVIDER_CREDENTIAL_REFERENCE)?;
        let session_token_ref = secret_ref(SESSION_TOKEN_REFERENCE)?;
        let remove = BTreeSet::from(["NAN_API_KEY".to_owned(), "VERCEL_OIDC_TOKEN".to_owned()]);

        Ok(LaunchPlan {
            schema_version: 2,
            launch_id: context.launch_id.clone(),
            harness: context.harness.clone(),
            model: context.model.clone(),
            web_search_policy: context.web_search_policy,
            transport: Transport::FxGatewayBridge {
                listen: ListenAddress {
                    host: "127.0.0.1".to_owned(),
                    port: 0,
                },
                provider_credential_ref,
                session_token_ref: session_token_ref.clone(),
            },
            process: ProcessSpec {
                arguments: context.user_arguments.clone(),
                working_directory: context.working_directory.clone(),
                terminal: TerminalMode::Inherit,
                forward_signals: true,
                preserve_exit_code: true,
            },
            environment: EnvironmentOverlay {
                public: BTreeMap::from([
                    ("FX_MODEL".to_owned(), context.model.resolved_id.clone()),
                    ("FX_SKIP_ONBOARDING".to_owned(), "1".to_owned()),
                    (
                        "FX_GATEWAY_BASE_URL".to_owned(),
                        BRIDGE_BASE_URL_PLACEHOLDER.to_owned(),
                    ),
                    (
                        "FX_GATEWAY_CHAT_URL".to_owned(),
                        FX_GATEWAY_CHAT_URL_PLACEHOLDER.to_owned(),
                    ),
                ]),
                secrets: BTreeMap::from([(
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                    session_token_ref,
                )]),
                remove,
            },
            temporary_artifacts: Vec::new(),
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
                    SESSION_TOKEN_ENVIRONMENT.to_owned(),
                    "FX_GATEWAY_BASE_URL".to_owned(),
                    "FX_GATEWAY_CHAT_URL".to_owned(),
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

fn validate_user_arguments(arguments: &[String]) -> Result<(), PlanError> {
    if let Some(argument) = arguments.iter().find(|argument| {
        matches!(argument.as_str(), "--model" | "-m")
            || argument.starts_with("--model=")
            || argument.starts_with("-m=")
    }) {
        return Err(PlanError::InvalidField {
            field: "process.arguments",
            message: format!("argument '{argument}' conflicts with nan-harness routing"),
        });
    }
    Ok(())
}
