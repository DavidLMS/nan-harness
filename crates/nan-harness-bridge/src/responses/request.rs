mod input;
mod reasoning;
#[cfg(test)]
mod tests;
mod tool_choice;
mod tools;
mod validation;
mod wire;

use crate::error::ApiError;
use nan_harness_core::model::CodingModelProfile;
use serde_json::{Map, Value, json};

pub(crate) use tools::{ToolCatalog, ToolTarget};
pub(crate) use wire::ResponsesRequest;

#[derive(Debug)]
pub(crate) struct TranslatedRequest {
    pub(crate) body: Value,
    pub(crate) tools: ToolCatalog,
}

pub(crate) fn translate(
    request: ResponsesRequest,
    model: &CodingModelProfile,
) -> Result<TranslatedRequest, ApiError> {
    validation::validate_request(&request)?;

    let (tools, catalog) = tools::translate_tools(&request.tools)?;
    let reasoning = reasoning::validate_reasoning(request.reasoning.as_ref(), model)?;
    let messages = input::translate(&request.instructions, request.input, &catalog)?;

    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(model.id.clone())),
        ("messages".to_owned(), Value::Array(messages)),
        ("stream".to_owned(), Value::Bool(true)),
        ("stream_options".to_owned(), json!({"include_usage": true})),
        (
            "max_tokens".to_owned(),
            Value::Number(model.max_output_tokens.into()),
        ),
    ]);
    reasoning::apply_reasoning_parameter(&mut body, &model.id, reasoning);
    if !tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(tools));
        body.insert(
            "tool_choice".to_owned(),
            tool_choice::translate_tool_choice(&request.tool_choice, &catalog),
        );
        body.insert(
            "parallel_tool_calls".to_owned(),
            Value::Bool(request.parallel_tool_calls),
        );
    }

    Ok(TranslatedRequest {
        body: Value::Object(body),
        tools: catalog,
    })
}
