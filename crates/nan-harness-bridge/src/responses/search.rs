use crate::error::ApiError;
use crate::upstream::NanClient;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub(crate) struct SearchReferences {
    urls: Mutex<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct NanSearchResponse {
    #[serde(default)]
    results: Vec<NanSearchResult>,
}

#[derive(Debug, Deserialize)]
struct NanSearchResult {
    title: String,
    url: String,
    #[serde(default)]
    snippet: String,
}

pub(crate) async fn execute(
    client: &NanClient,
    references: &Arc<SearchReferences>,
    request: Value,
) -> Result<Value, ApiError> {
    let query = search_query(&request, references);
    let count = result_count(&request);
    let response = client
        .search(&json!({
            "query": query,
            "count": count,
            "fetch_content": false
        }))
        .await?;
    let response = ensure_success(response)?;
    let response = response
        .json::<NanSearchResponse>()
        .await
        .map_err(|error| ApiError::InvalidUpstream(error.to_string()))?;
    let allowed_domains = allowed_domains(&request);
    let results = response
        .results
        .into_iter()
        .filter(|result| valid_result(result, &allowed_domains))
        .take(count)
        .collect::<Vec<_>>();
    let structured = results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let reference = format!("turn0search{index}");
            references
                .urls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(reference.clone(), result.url.clone());
            json!({
                "type": "text_result",
                "ref_id": reference,
                "url": result.url,
                "title": limited(&result.title, 500),
                "snippet": limited(&result.snippet, 2_000)
            })
        })
        .collect::<Vec<_>>();
    let output = if results.is_empty() {
        "No web search results were found.".to_owned()
    } else {
        results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "{}. {}\nURL: {}\n{}",
                    index + 1,
                    limited(&result.title, 500),
                    result.url,
                    limited(&result.snippet, 2_000)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Ok(json!({
        "encrypted_output": null,
        "output": output,
        "results": structured
    }))
}

fn search_query(request: &Value, references: &SearchReferences) -> String {
    let commands = request.get("commands").unwrap_or(&Value::Null);
    for key in ["search_query", "image_query"] {
        let queries = commands
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|query| query.get("q").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !queries.is_empty() {
            return queries.join(" OR ");
        }
    }
    for key in ["open", "find", "click", "screenshot"] {
        if let Some(reference) = commands
            .get(key)
            .and_then(Value::as_array)
            .and_then(|operations| operations.first())
            .and_then(|operation| operation.get("ref_id"))
            .and_then(Value::as_str)
        {
            return references
                .urls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(reference)
                .cloned()
                .unwrap_or_else(|| reference.to_owned());
        }
    }
    commands.to_string()
}

fn result_count(request: &Value) -> usize {
    match request
        .pointer("/commands/response_length")
        .and_then(Value::as_str)
    {
        Some("long") => 20,
        Some("medium") => 10,
        _ => 5,
    }
}

fn allowed_domains(request: &Value) -> Vec<&str> {
    request
        .pointer("/settings/filters/allowed_domains")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn valid_result(result: &NanSearchResult, allowed_domains: &[&str]) -> bool {
    let Ok(url) = Url::parse(&result.url) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    allowed_domains.is_empty()
        || allowed_domains.iter().any(|domain| {
            url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case(domain)
                    || host
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{}", domain.to_ascii_lowercase()))
            })
        })
}

fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(ApiError::UpstreamStatus {
        status,
        message: "NaN web search failed".to_owned(),
    })
}

fn limited(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}
