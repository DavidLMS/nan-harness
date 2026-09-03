use nan_harness_core::CodingModelProfile;
use std::fmt::Write as _;

use super::reasoning_capable;

pub(in crate::prepared) fn deepseek_model_catalog(
    models: &[CodingModelProfile],
) -> Result<String, String> {
    let mut output = String::new();
    for model in models {
        let id = serde_json::to_string(&model.id)
            .map_err(|error| format!("could not serialize a NaN model ID: {error}"))?;
        let name = serde_json::to_string(&model.display_name)
            .map_err(|error| format!("could not serialize a NaN model name: {error}"))?;
        let input = if model.image_input {
            "[text, image]"
        } else {
            "[text]"
        };
        write!(
            output,
            "          - id: {id}\n            name: {name}\n            contextWindow: {}\n            maxTokens: {}\n            input: {input}\n            reasoning: {}\n",
            model.context_window,
            model.max_output_tokens,
            reasoning_capable(model.reasoning)
        )
        .map_err(|error| format!("could not render the DeepSeek model catalog: {error}"))?;
    }
    Ok(output)
}

pub(in crate::prepared) fn kimi_code_model_catalog(
    models: &[CodingModelProfile],
    selected_model_id: &str,
) -> Result<String, String> {
    let model_tables: toml::map::Map<String, toml::Value> = models
        .iter()
        .filter(|model| model.id != selected_model_id)
        .map(kimi_model_table)
        .collect::<Result<_, _>>()?;
    render_kimi_model_catalog(model_tables)
}

pub(in crate::prepared) fn kimi_model_table(
    model: &CodingModelProfile,
) -> Result<(String, toml::Value), String> {
    let (context_window, max_output_tokens) = kimi_model_limits(model)?;
    let model_config = toml::Table::from_iter([
        (
            "capabilities".to_owned(),
            toml::Value::Array(kimi_model_capabilities(model)),
        ),
        (
            "display_name".to_owned(),
            toml::Value::String(model.display_name.clone()),
        ),
        (
            "max_context_size".to_owned(),
            toml::Value::Integer(context_window),
        ),
        (
            "max_output_size".to_owned(),
            toml::Value::Integer(max_output_tokens),
        ),
        ("model".to_owned(), toml::Value::String(model.id.clone())),
        (
            "provider".to_owned(),
            toml::Value::String("__kimi_env__".to_owned()),
        ),
    ]);
    Ok((
        format!("nan/{}", model.id),
        toml::Value::Table(model_config),
    ))
}

pub(in crate::prepared) fn kimi_model_limits(
    model: &CodingModelProfile,
) -> Result<(i64, i64), String> {
    let context_window = i64::try_from(model.context_window)
        .map_err(|_| format!("model '{}' context window is too large for TOML", model.id))?;
    let max_output_tokens = i64::try_from(model.max_output_tokens)
        .map_err(|_| format!("model '{}' output limit is too large for TOML", model.id))?;
    Ok((context_window, max_output_tokens))
}

pub(in crate::prepared) fn kimi_model_capabilities(model: &CodingModelProfile) -> Vec<toml::Value> {
    match (model.image_input, reasoning_capable(model.reasoning)) {
        (true, true) => vec![
            toml::Value::String("image_in".to_owned()),
            toml::Value::String("thinking".to_owned()),
        ],
        (true, false) => vec![toml::Value::String("image_in".to_owned())],
        (false, true) => vec![toml::Value::String("thinking".to_owned())],
        (false, false) => Vec::new(),
    }
}

pub(in crate::prepared) fn render_kimi_model_catalog(
    model_tables: toml::map::Map<String, toml::Value>,
) -> Result<String, String> {
    toml::to_string(&toml::Value::Table(toml::Table::from_iter([(
        "models".to_owned(),
        toml::Value::Table(model_tables),
    )])))
    .map_err(|error| format!("could not render the Kimi Code model catalog: {error}"))
}
