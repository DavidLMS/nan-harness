use crate::consent::InstallationId;
use crate::event::{
    Architecture, HarnessKind, OperationKind, OsFamily, RuntimeContext, TargetEnvironment,
    Transport,
};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use std::net::IpAddr;
use std::time::Duration;
use thiserror::Error;
use url::Url;

pub const DEFAULT_USAGE_EXPORT_TIMEOUT: Duration = Duration::from_millis(1_200);

const EVENT_NAME: &str = "nan-invoked";
const EVENT_HOSTNAME: &str = "nan-harness.cli";
const EVENT_PATH: &str = "/cli";
const INGESTION_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageEvent {
    harness: Option<HarnessKind>,
    operation: OperationKind,
    transport: Option<Transport>,
}

impl UsageEvent {
    #[must_use]
    pub const fn new(
        harness: Option<HarnessKind>,
        operation: OperationKind,
        transport: Option<Transport>,
    ) -> Self {
        Self {
            harness,
            operation,
            transport,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UmamiExporter {
    endpoint: Url,
    website_id: String,
    client: Client,
}

impl UmamiExporter {
    /// Creates a bounded exporter for an Umami installation.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError`] when the endpoint or website identifier is invalid, or when the
    /// HTTP client cannot be built.
    pub fn new(
        base_url: &str,
        website_id: &str,
        timeout: Duration,
    ) -> Result<Self, AnalyticsError> {
        if !valid_website_id(website_id) {
            return Err(AnalyticsError::InvalidWebsiteId);
        }
        let base_url = Url::parse(base_url).map_err(AnalyticsError::InvalidEndpoint)?;
        validate_endpoint(&base_url)?;
        let endpoint = base_url
            .join("api/send")
            .map_err(AnalyticsError::InvalidEndpoint)?;
        let client = Client::builder()
            .timeout(timeout)
            .user_agent(INGESTION_USER_AGENT)
            .build()
            .map_err(AnalyticsError::BuildClient)?;
        Ok(Self {
            endpoint,
            website_id: website_id.to_owned(),
            client,
        })
    }

    /// Sends one allowlisted invocation event.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyticsError`] when Umami cannot be reached or rejects the event.
    pub async fn export(
        &self,
        installation_id: &InstallationId,
        event: UsageEvent,
    ) -> Result<(), AnalyticsError> {
        let runtime = RuntimeContext::current(false);
        let request = UmamiRequest {
            event_type: "event",
            payload: UmamiPayload {
                website: &self.website_id,
                hostname: EVENT_HOSTNAME,
                url: EVENT_PATH,
                name: EVENT_NAME,
                id: installation_id.as_str(),
                data: UsageEventData {
                    nan_version: env!("CARGO_PKG_VERSION"),
                    harness: event.harness,
                    operation: event.operation,
                    transport: event.transport,
                    os_family: runtime.os_family(),
                    architecture: runtime.architecture(),
                    target_environment: runtime.target_environment(),
                },
            },
        };
        let response = self
            .client
            .post(self.endpoint.clone())
            .json(&request)
            .send()
            .await
            .map_err(AnalyticsError::Request)?;
        if !response.status().is_success() {
            return Err(AnalyticsError::Rejected(response.status()));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UmamiRequest<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    payload: UmamiPayload<'a>,
}

#[derive(Debug, Serialize)]
struct UmamiPayload<'a> {
    website: &'a str,
    hostname: &'static str,
    url: &'static str,
    name: &'static str,
    id: &'a str,
    data: UsageEventData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageEventData {
    nan_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<HarnessKind>,
    operation: OperationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<Transport>,
    os_family: OsFamily,
    architecture: Architecture,
    target_environment: TargetEnvironment,
}

fn validate_endpoint(endpoint: &Url) -> Result<(), AnalyticsError> {
    if endpoint.cannot_be_a_base()
        || endpoint.username() != ""
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/"
    {
        return Err(AnalyticsError::UnsupportedEndpoint);
    }
    if endpoint.scheme() == "https" || endpoint_is_loopback(endpoint) {
        Ok(())
    } else {
        Err(AnalyticsError::InsecureEndpoint)
    }
}

fn endpoint_is_loopback(endpoint: &Url) -> bool {
    endpoint.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn valid_website_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[derive(Debug, Error)]
pub enum AnalyticsError {
    #[error("Umami endpoint is not a valid URL: {0}")]
    InvalidEndpoint(url::ParseError),
    #[error("Umami endpoint must be an origin URL without credentials, a path, query, or fragment")]
    UnsupportedEndpoint,
    #[error("Umami endpoint must use HTTPS unless it is a loopback address")]
    InsecureEndpoint,
    #[error("Umami website ID must be a UUID")]
    InvalidWebsiteId,
    #[error("could not build the Umami HTTP client: {0}")]
    BuildClient(reqwest::Error),
    #[error("could not send usage analytics: {0}")]
    Request(reqwest::Error),
    #[error("Umami rejected usage analytics with HTTP {0}")]
    Rejected(StatusCode),
}
