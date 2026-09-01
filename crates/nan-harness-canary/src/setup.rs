use crate::app::SetupArgs;
use crate::credentials::{self, API_KEY_ACCOUNT, NTFY_TOKEN_ACCOUNT};
use reqwest::header::ACCEPT;
use serde::Deserialize;
use std::env;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use url::Url;
use zeroize::Zeroizing;

const API_KEY_ENVIRONMENT_VARIABLE: &str = "NAN_API_KEY";
const NTFY_TOKEN_ENVIRONMENT_VARIABLE: &str = "NAN_CANARY_NTFY_TOKEN";
const REQUIRED_MODEL: &str = "qwen3.6";
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) async fn run(arguments: &SetupArgs) -> Result<(), SetupError> {
    let api_key = Zeroizing::new(read_api_key()?);
    let models = discover_models(&arguments.provider_base_url, &api_key).await?;
    if !models.iter().any(|model| model == REQUIRED_MODEL) {
        return Err(SetupError::MissingRequiredModel(REQUIRED_MODEL));
    }

    check_command("cargo", &["--version"]).await?;
    check_command("gh", &["auth", "status"]).await?;
    check_command("jq", &["--version"]).await?;
    check_command("ssh", &["-V"]).await?;
    check_command("/usr/bin/security", &["list-keychains"]).await?;
    if !arguments.skip_tart {
        check_command("tart", &["--version"]).await?;
        check_command("sshpass", &["-V"]).await?;
    }

    if !arguments.check_only {
        credentials::store(API_KEY_ACCOUNT, &api_key)
            .await
            .map_err(SetupError::StoreCredential)?;
    }

    if let Some(ntfy_url) = &arguments.ntfy_url {
        validate_ntfy_url(ntfy_url)?;
        let token = Zeroizing::new(read_required_environment(
            NTFY_TOKEN_ENVIRONMENT_VARIABLE,
            SetupError::MissingNtfyToken,
            SetupError::NonUnicodeNtfyToken,
        )?);
        if !arguments.check_only {
            credentials::store(NTFY_TOKEN_ACCOUNT, &token)
                .await
                .map_err(SetupError::StoreCredential)?;
        }
    }

    println!(
        "Canary setup is valid: {} provider models discovered and '{}' is available.",
        models.len(),
        REQUIRED_MODEL
    );
    if arguments.check_only {
        println!("The existing NAN_API_KEY was not written to Keychain.");
    } else {
        println!("The existing NAN_API_KEY is available to the local canary through Keychain.");
    }
    if arguments.ntfy_url.is_some() {
        println!("The private ntfy configuration is valid.");
    }
    Ok(())
}

fn read_api_key() -> Result<String, SetupError> {
    read_required_environment(
        API_KEY_ENVIRONMENT_VARIABLE,
        SetupError::MissingApiKey,
        SetupError::NonUnicodeApiKey,
    )
}

fn read_required_environment(
    name: &str,
    missing: SetupError,
    non_unicode: SetupError,
) -> Result<String, SetupError> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) | Err(env::VarError::NotPresent) => Err(missing),
        Err(env::VarError::NotUnicode(_)) => Err(non_unicode),
    }
}

fn validate_ntfy_url(value: &str) -> Result<(), SetupError> {
    let url = Url::parse(value).map_err(SetupError::InvalidNtfyUrl)?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(SetupError::InsecureNtfyUrl);
    }
    Ok(())
}

async fn discover_models(base_url: &str, api_key: &str) -> Result<Vec<String>, SetupError> {
    let endpoint = models_endpoint(base_url)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(PROVIDER_TIMEOUT)
        .build()
        .map_err(SetupError::BuildClient)?;
    let response = client
        .get(endpoint)
        .header(ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(SetupError::ProviderRequest)?;
    let status = response.status();
    if !status.is_success() {
        return Err(SetupError::ProviderStatus(status));
    }
    let body = response
        .bytes()
        .await
        .map_err(SetupError::ProviderResponse)?;
    let models = parse_models(&body)?;
    if models.is_empty() {
        return Err(SetupError::EmptyModelCatalog);
    }
    Ok(models)
}

fn parse_models(body: &[u8]) -> Result<Vec<String>, SetupError> {
    let response: ModelsResponse =
        serde_json::from_slice(body).map_err(SetupError::InvalidProviderResponse)?;
    Ok(nan_harness_core::coding_models_from_provider_ids(
        response.data.into_iter().map(|model| model.id),
    )
    .into_iter()
    .map(|model| model.id)
    .collect())
}

fn models_endpoint(base_url: &str) -> Result<Url, SetupError> {
    let mut url = Url::parse(base_url).map_err(SetupError::InvalidProviderUrl)?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(SetupError::InsecureProviderUrl);
    }
    let path = format!("{}/models", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

async fn check_command(program: &'static str, arguments: &[&str]) -> Result<(), SetupError> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|source| SetupError::MissingCommand { program, source })?;
    if !output.status.success() {
        return Err(SetupError::CommandFailed(program));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Debug, Deserialize)]
struct Model {
    id: String,
}

#[derive(Debug, Error)]
pub(crate) enum SetupError {
    #[error("NAN_API_KEY is not set in the current environment")]
    MissingApiKey,
    #[error("NAN_API_KEY is not valid Unicode")]
    NonUnicodeApiKey,
    #[error("NAN_CANARY_NTFY_TOKEN is not set in the current environment")]
    MissingNtfyToken,
    #[error("NAN_CANARY_NTFY_TOKEN is not valid Unicode")]
    NonUnicodeNtfyToken,
    #[error("the provider base URL is invalid: {0}")]
    InvalidProviderUrl(url::ParseError),
    #[error("the provider base URL must use HTTPS, except for localhost tests")]
    InsecureProviderUrl,
    #[error("the private ntfy URL is invalid: {0}")]
    InvalidNtfyUrl(url::ParseError),
    #[error("the private ntfy URL must use HTTPS")]
    InsecureNtfyUrl,
    #[error("could not build the provider client: {0}")]
    BuildClient(reqwest::Error),
    #[error("could not query the NaN model catalog: {0}")]
    ProviderRequest(reqwest::Error),
    #[error("the NaN model catalog returned HTTP {0}")]
    ProviderStatus(reqwest::StatusCode),
    #[error("the NaN model catalog response is invalid: {0}")]
    ProviderResponse(reqwest::Error),
    #[error("the NaN model catalog response is invalid: {0}")]
    InvalidProviderResponse(serde_json::Error),
    #[error("the NaN model catalog is empty")]
    EmptyModelCatalog,
    #[error("the required canary model '{0}' is not available")]
    MissingRequiredModel(&'static str),
    #[error("required command '{program}' is unavailable: {source}")]
    MissingCommand {
        program: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("required command '{0}' returned a failure status")]
    CommandFailed(&'static str),
    #[error("could not store the canary credential: {0}")]
    StoreCredential(credentials::CredentialError),
}

#[cfg(test)]
mod tests {
    use super::{SetupError, models_endpoint, parse_models};

    #[test]
    fn provider_endpoint_requires_https_outside_localhost() {
        assert!(matches!(
            models_endpoint("http://api.nan.builders/v1"),
            Err(SetupError::InsecureProviderUrl)
        ));
        assert_eq!(
            models_endpoint("https://api.nan.builders/v1/")
                .expect("provider URL should be valid")
                .as_str(),
            "https://api.nan.builders/v1/models"
        );
    }

    #[test]
    fn model_catalog_payload_is_normalized_without_credentials() {
        let models =
            parse_models(br#"{"data":[{"id":"qwen3.6"},{"id":"minimax-h3"},{"id":"whisper"}]}"#)
                .expect("models should be parsed");
        assert_eq!(models, ["qwen3.6"]);
    }
}
