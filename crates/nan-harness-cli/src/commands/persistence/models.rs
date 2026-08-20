use super::PersistenceError;
use jsonc_parser::cst::CstInputValue;
use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{CodingModelProfile, coding_models_from_provider_ids};
use nan_harness_runtime::ResolvedConfig;
use nan_harness_runtime::config::DEFAULT_PROVIDER_BASE_URL;
use reqwest::header::ACCEPT;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::time::Duration;
use url::Url;

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
    let response = request
        .send()
        .await
        .map_err(PersistenceError::DiscoverModels)?;
    let status = response.status();
    if !status.is_success() {
        return Err(PersistenceError::ModelDiscoveryStatus(status.as_u16()));
    }
    let payload = response
        .json::<NanModelsResponse>()
        .await
        .map_err(PersistenceError::ParseModels)?;
    let models = coding_models_from_provider_ids(payload.data.into_iter().map(|model| model.id));
    if models.is_empty() {
        return Err(PersistenceError::NoModels);
    }
    Ok(models)
}

pub(super) fn qwen_code_provider(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> CstInputValue {
    CstInputValue::Array(
        models
            .iter()
            .map(|model| {
                let mut generation_config = vec![
                    (
                        "contextWindowSize".to_owned(),
                        CstInputValue::Number(model.context_window.to_string()),
                    ),
                    (
                        "modalities".to_owned(),
                        CstInputValue::Object(vec![(
                            "image".to_owned(),
                            CstInputValue::Bool(model.image_input),
                        )]),
                    ),
                    (
                        "samplingParams".to_owned(),
                        CstInputValue::Object(vec![(
                            "max_tokens".to_owned(),
                            CstInputValue::Number(model.max_output_tokens.to_string()),
                        )]),
                    ),
                ];
                if matches!(model.reasoning, ReasoningPolicy::Unsupported) {
                    generation_config.push(("reasoning".to_owned(), CstInputValue::Bool(false)));
                }
                CstInputValue::Object(vec![
                    (
                        "baseUrl".to_owned(),
                        CstInputValue::String(provider_base_url.to_owned()),
                    ),
                    (
                        "description".to_owned(),
                        CstInputValue::String(model.description.clone()),
                    ),
                    (
                        "envKey".to_owned(),
                        CstInputValue::String("NAN_API_KEY".to_owned()),
                    ),
                    (
                        "generationConfig".to_owned(),
                        CstInputValue::Object(generation_config),
                    ),
                    ("id".to_owned(), CstInputValue::String(model.id.clone())),
                    (
                        "name".to_owned(),
                        CstInputValue::String(model.display_name.clone()),
                    ),
                ])
            })
            .collect(),
    )
}

pub(super) fn deepseek_provider_settings(
    models: &[CodingModelProfile],
    provider_base_url: &str,
) -> Result<String, PersistenceError> {
    let base_url =
        serde_json::to_string(provider_base_url).map_err(PersistenceError::SerializeProvider)?;
    let mut output = format!(
        "llm-pi-ai:\n  providers:\n    nan-harness:\n      displayName: NaN\n      apiKeyEnv: NAN_API_KEY\n      api: openai-completions\n      baseURL: {base_url}\n      models:\n"
    );
    for model in models {
        let id = serde_json::to_string(&model.id).map_err(PersistenceError::SerializeProvider)?;
        let name = serde_json::to_string(&model.display_name)
            .map_err(PersistenceError::SerializeProvider)?;
        let input = if model.image_input {
            "[text, image]"
        } else {
            "[text]"
        };
        write!(
            output,
            "        - id: {id}\n          name: {name}\n          reasoning: {}\n          contextWindow: {}\n          maxTokens: {}\n          input: {input}\n          compat:\n            supportsReasoningEffort: {}\n",
            !matches!(
                model.reasoning,
                ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown
            ),
            model.context_window,
            model.max_output_tokens,
            matches!(model.reasoning, ReasoningPolicy::Effort { .. })
        )
        .map_err(|error| PersistenceError::RenderConfiguration(error.to_string()))?;
    }
    Ok(output)
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
                ]),
            )
        })
        .collect()
}

pub(super) fn validate_provider_url(value: &str) -> Result<(), PersistenceError> {
    let url = Url::parse(value).map_err(PersistenceError::InvalidProviderUrl)?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
        Ok(())
    } else {
        Err(PersistenceError::UnsupportedProviderUrl)
    }
}

pub(crate) fn effective_provider_base_url(explicit: Option<&str>) -> String {
    explicit
        .map(ToOwned::to_owned)
        .or_else(|| env::var("NAN_BASE_URL").ok())
        .unwrap_or_else(|| DEFAULT_PROVIDER_BASE_URL.to_owned())
}
