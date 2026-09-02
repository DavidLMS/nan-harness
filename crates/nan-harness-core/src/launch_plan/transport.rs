use crate::secret::SecretRef;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
    FxGatewayBridge,
}

impl fmt::Display for TransportKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::DirectChat => "direct-chat",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ResponsesBridge => "responses-bridge",
            Self::FxGatewayBridge => "fx-gateway-bridge",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    ChatCompletions,
    AnthropicMessages,
    OpenAiResponses,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenAddress {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Transport {
    DirectChat {
        protocol: Protocol,
        base_url: String,
        credential_target: String,
    },
    AnthropicBridge {
        client_protocol: Protocol,
        upstream_protocol: Protocol,
        listen: ListenAddress,
        provider_credential_ref: SecretRef,
        session_token_ref: SecretRef,
    },
    ResponsesBridge {
        client_protocol: Protocol,
        upstream_protocol: Protocol,
        listen: ListenAddress,
        provider_credential_ref: SecretRef,
        session_token_ref: SecretRef,
    },
    FxGatewayBridge {
        listen: ListenAddress,
        provider_credential_ref: SecretRef,
        session_token_ref: SecretRef,
    },
}

impl Transport {
    #[must_use]
    pub const fn kind(&self) -> TransportKind {
        match self {
            Self::DirectChat { .. } => TransportKind::DirectChat,
            Self::AnthropicBridge { .. } => TransportKind::AnthropicBridge,
            Self::ResponsesBridge { .. } => TransportKind::ResponsesBridge,
            Self::FxGatewayBridge { .. } => TransportKind::FxGatewayBridge,
        }
    }

    #[must_use]
    pub const fn is_bridge(&self) -> bool {
        !matches!(self, Self::DirectChat { .. })
    }
}
