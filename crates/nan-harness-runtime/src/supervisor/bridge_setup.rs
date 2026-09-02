use super::RuntimeError;
use nan_harness_core::launch_plan::ListenAddress;
use nan_harness_core::{CodingModelProfile, SecretValue};
use std::fmt::Write as _;
use tokio::net::TcpListener;

#[derive(Clone, Copy)]
pub(super) struct BridgeLaunchOptions<'a> {
    pub(super) discovered_models: &'a [CodingModelProfile],
    pub(super) web_search_enabled: bool,
}

pub(super) struct BoundBridgeEndpoint {
    pub(super) listener: TcpListener,
    pub(super) base_url: String,
}

impl BoundBridgeEndpoint {
    pub(super) async fn bind_transport(listen: &ListenAddress) -> Result<Self, RuntimeError> {
        let listener = TcpListener::bind((listen.host.as_str(), listen.port))
            .await
            .map_err(RuntimeError::BindBridge)?;
        Self::from_listener(listener)
    }

    pub(super) async fn bind_direct_chat_gateway() -> Result<Self, RuntimeError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(RuntimeError::BindBridge)?;
        Self::from_listener(listener)
    }

    fn from_listener(listener: TcpListener) -> Result<Self, RuntimeError> {
        let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
        Ok(Self {
            listener,
            base_url: format!("http://{address}"),
        })
    }
}

pub(super) fn generate_session_token() -> Result<SecretValue, RuntimeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(RuntimeError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    SecretValue::new(token).map_err(RuntimeError::Secret)
}
