use super::PersistenceError;
use super::helpers::jsonc_input;
use jsonc_parser::cst::CstInputValue;
use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{CodingModelProfile, coding_models_from_provider_ids};
use nan_harness_runtime::ResolvedConfig;
use reqwest::header::ACCEPT;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

const MAX_MODELS_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct NanModelsResponse {
    data: Vec<NanModel>,
}

#[derive(Debug, Deserialize)]
struct NanModel {
    id: String,
}

pub(crate) async fn discover_models(
    config: &ResolvedConfig,
) -> Result<Vec<CodingModelProfile>, PersistenceError> {
    let result = discover_models_live(config).await;
    config
        .secrets
        .with_secret(&config.provider_credential_ref, |secret| {
            nan_harness_runtime::model_discovery::resolve_model_discovery(
                &config.provider_base_url,
                secret,
                result,
                fallback_reason,
            )
        })
        .map_err(PersistenceError::Secret)?
        .map(nan_harness_runtime::model_discovery::ModelDiscovery::into_models_with_notice)
}

pub(crate) fn fallback_reason(
    error: &PersistenceError,
) -> Option<nan_harness_runtime::model_discovery::ModelFallbackReason> {
    use nan_harness_runtime::model_discovery::ModelFallbackReason;
    match error {
        PersistenceError::DiscoverModels(error) if error.is_timeout() => {
            Some(ModelFallbackReason::Timeout)
        }
        PersistenceError::DiscoverModels(_) => Some(ModelFallbackReason::Transport),
        PersistenceError::ModelDiscoveryStatus(status) => ModelFallbackReason::from_status(*status),
        PersistenceError::ParseModels(_) | PersistenceError::ModelDiscoveryTooLarge => {
            Some(ModelFallbackReason::InvalidResponse)
        }
        PersistenceError::NoModels => Some(ModelFallbackReason::NoModels),
        _ => None,
    }
}

pub(crate) async fn discover_models_live(
    config: &ResolvedConfig,
) -> Result<Vec<CodingModelProfile>, PersistenceError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(PersistenceError::BuildClient)?;
    let endpoint = format!("{}/models", config.provider_base_url.trim_end_matches('/'));
    let request = config
        .secrets
        .with_secret(&config.provider_credential_ref, |api_key| {
            client
                .get(endpoint)
                .header(ACCEPT, "application/json")
                .bearer_auth(api_key)
        })
        .map_err(PersistenceError::Secret)?;
    let mut response = request
        .send()
        .await
        .map_err(PersistenceError::DiscoverModels)?;
    let status = response.status();
    if !status.is_success() {
        return Err(PersistenceError::ModelDiscoveryStatus(status.as_u16()));
    }
    let body = read_bounded_models_response(&mut response).await?;
    let payload = serde_json::from_slice::<NanModelsResponse>(&body)
        .map_err(PersistenceError::ParseModels)?;
    let models = coding_models_from_provider_ids(payload.data.into_iter().map(|model| model.id));
    if models.is_empty() {
        return Err(PersistenceError::NoModels);
    }
    Ok(models)
}

async fn read_bounded_models_response(
    response: &mut reqwest::Response,
) -> Result<Vec<u8>, PersistenceError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MODELS_RESPONSE_BYTES as u64)
    {
        return Err(PersistenceError::ModelDiscoveryTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(PersistenceError::DiscoverModels)?
    {
        let next_len = body.len().saturating_add(chunk.len());
        if next_len > MAX_MODELS_RESPONSE_BYTES {
            return Err(PersistenceError::ModelDiscoveryTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(super) fn qwen_code_provider(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> CstInputValue {
    let mut models = nan_harness_runtime::qwen_code_model_catalog(models, provider_base_url);
    if let Some(entries) = models.as_array_mut() {
        for entry in entries {
            entry["envKey"] = serde_json::json!("NAN_API_KEY");
        }
    }
    jsonc_input(&models)
}

pub(super) fn deepseek_provider_settings(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> Result<String, PersistenceError> {
    let selected = models
        .iter()
        .find(|model| model.id == "qwen3.6")
        .or_else(|| models.first())
        .ok_or(PersistenceError::NoModels)?;
    let patch = serde_json::json!([
        {"id": "agent-default-model", "config": {
            "provider": nan_harness_runtime::deepseek_provider_for(selected), "model": selected.id,
        }},
        {"id": "llm-pi-ai", "config": {"providers":
            nan_harness_runtime::deepseek_provider_catalog(models, provider_base_url)}},
    ]);
    serde_yaml_ng::to_string(&patch)
        .map_err(|error| PersistenceError::RenderConfiguration(error.to_string()))
}

pub(super) fn aider_model_settings(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> Result<String, PersistenceError> {
    let api_base =
        serde_json::to_string(provider_base_url).map_err(PersistenceError::SerializeProvider)?;
    let mut output = String::new();
    for model in models {
        let name = serde_json::to_string(&format!("nan/{}", model.id))
            .map_err(PersistenceError::SerializeProvider)?;
        let upstream = serde_json::to_string(&format!("openai/{}", model.id))
            .map_err(PersistenceError::SerializeProvider)?;
        write!(
            output,
            "- name: {name}\n  edit_format: diff\n  editor_model_name: {name}\n  use_repo_map: true\n  weak_model_name: {name}\n  extra_params:\n    model: {upstream}\n    api_key: os.environ/NAN_API_KEY\n    api_base: {api_base}\n"
        )
        .map_err(|error| PersistenceError::RenderConfiguration(error.to_string()))?;
        if matches!(model.reasoning, ReasoningPolicy::Effort { .. }) {
            output.push_str("  accepts_settings: [reasoning_effort]\n");
        }
    }
    Ok(output)
}

pub(super) fn aider_model_metadata(
    models: &[CodingModelProfile],
) -> BTreeMap<String, CstInputValue> {
    models
        .iter()
        .map(|model| {
            (
                format!("nan/{}", model.id),
                CstInputValue::Object(vec![
                    (
                        "litellm_provider".to_owned(),
                        CstInputValue::String("openai".to_owned()),
                    ),
                    (
                        "max_input_tokens".to_owned(),
                        CstInputValue::Number(model.context_window.to_string()),
                    ),
                    (
                        "max_output_tokens".to_owned(),
                        CstInputValue::Number(model.max_output_tokens.to_string()),
                    ),
                    (
                        "max_tokens".to_owned(),
                        CstInputValue::Number(model.max_output_tokens.to_string()),
                    ),
                    ("mode".to_owned(), CstInputValue::String("chat".to_owned())),
                    (
                        "supports_function_calling".to_owned(),
                        CstInputValue::Bool(true),
                    ),
                    (
                        "supports_vision".to_owned(),
                        CstInputValue::Bool(model.image_input),
                    ),
                    (
                        "supports_reasoning".to_owned(),
                        CstInputValue::Bool(!matches!(
                            model.reasoning,
                            ReasoningPolicy::Unknown | ReasoningPolicy::Unsupported
                        )),
                    ),
                ]),
            )
        })
        .collect()
}
