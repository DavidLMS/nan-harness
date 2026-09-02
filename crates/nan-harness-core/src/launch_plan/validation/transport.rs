use super::invalid;
use crate::error::PlanError;
use crate::harness::HarnessKind;
use crate::launch_plan::{
    EnvironmentOverlay, LaunchPlan, ListenAddress, PROVIDER_BASE_URL_PLACEHOLDER, Protocol,
    Transport, TransportKind,
};
use crate::secret::SecretRef;

pub(super) fn validate(plan: &LaunchPlan) -> Result<(), PlanError> {
    let expected = match plan.harness.kind {
        HarnessKind::ClaudeCode => TransportKind::AnthropicBridge,
        HarnessKind::Codex => TransportKind::ResponsesBridge,
        HarnessKind::Fx => TransportKind::FxGatewayBridge,
        HarnessKind::OpenCode
        | HarnessKind::Hermes
        | HarnessKind::Pi
        | HarnessKind::Omp
        | HarnessKind::PrimeAgent
        | HarnessKind::DeepSeekHarness
        | HarnessKind::OpenClaw
        | HarnessKind::Cline
        | HarnessKind::QwenCode
        | HarnessKind::KimiCode
        | HarnessKind::Aider
        | HarnessKind::Goose => TransportKind::DirectChat,
    };
    let actual = plan.transport.kind();
    if actual != expected {
        return Err(PlanError::TransportMismatch {
            harness: plan.harness.kind,
            expected,
            actual,
        });
    }

    match &plan.transport {
        Transport::DirectChat {
            protocol,
            base_url,
            credential_target,
        } => {
            if protocol != &Protocol::ChatCompletions {
                return invalid(
                    "transport.protocol",
                    "direct transport requires chat-completions",
                );
            }
            if base_url != PROVIDER_BASE_URL_PLACEHOLDER && !is_http_url(base_url) {
                return invalid("transport.baseUrl", "must be an HTTP or HTTPS URL");
            }
            if !plan.environment.secrets.contains_key(credential_target) {
                return Err(PlanError::MissingSecretReference {
                    reference: credential_target.clone(),
                });
            }
        }
        Transport::AnthropicBridge {
            client_protocol,
            upstream_protocol,
            listen,
            session_token_ref,
            ..
        } => {
            validate_bridge_protocols(
                *client_protocol,
                *upstream_protocol,
                listen,
                Protocol::AnthropicMessages,
                Protocol::ChatCompletions,
            )?;
            validate_child_secret_ref(&plan.environment, session_token_ref)?;
        }
        Transport::ResponsesBridge {
            client_protocol,
            upstream_protocol,
            listen,
            session_token_ref,
            ..
        } => {
            validate_bridge_protocols(
                *client_protocol,
                *upstream_protocol,
                listen,
                Protocol::OpenAiResponses,
                Protocol::ChatCompletions,
            )?;
            validate_child_secret_ref(&plan.environment, session_token_ref)?;
        }
        Transport::FxGatewayBridge {
            listen,
            session_token_ref,
            ..
        } => {
            if listen.host != "127.0.0.1" {
                return invalid("transport.listen.host", "bridges must bind to 127.0.0.1");
            }
            validate_child_secret_ref(&plan.environment, session_token_ref)?;
        }
    }
    Ok(())
}

fn validate_bridge_protocols(
    client: Protocol,
    upstream: Protocol,
    listen: &ListenAddress,
    expected_client: Protocol,
    expected_upstream: Protocol,
) -> Result<(), PlanError> {
    if client != expected_client || upstream != expected_upstream {
        return invalid(
            "transport",
            "bridge protocols do not match the selected bridge",
        );
    }
    if listen.host != "127.0.0.1" {
        return invalid("transport.listen.host", "bridges must bind to 127.0.0.1");
    }
    Ok(())
}

fn validate_child_secret_ref(
    environment: &EnvironmentOverlay,
    reference: &SecretRef,
) -> Result<(), PlanError> {
    if environment.secrets.values().any(|value| value == reference) {
        Ok(())
    } else {
        Err(PlanError::MissingSecretReference {
            reference: reference.to_string(),
        })
    }
}

fn is_http_url(value: &str) -> bool {
    (value.starts_with("http://") || value.starts_with("https://"))
        && !value.chars().any(char::is_whitespace)
        && value
            .split_once("://")
            .is_some_and(|(_, rest)| !rest.is_empty())
}
