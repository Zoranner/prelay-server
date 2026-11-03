use serde_json::Value;

use crate::codec::internal::{InternalContentPart, InternalRequest, InternalTool};
use crate::error::AppError;

use super::input::decode_input;
pub use super::items::decode_items_for_chat_bridge;

pub fn decode_responses_request(value: &Value) -> Result<InternalRequest, AppError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("model 不能为空".to_string()))?
        .to_string();
    let stream = decode_optional_bool(value.get("stream"), "stream", false)?;
    let previous_response_id = decode_previous_response_id(value.get("previous_response_id"))?;
    let conversation = decode_conversation(value.get("conversation"))?;
    if previous_response_id.is_some() && conversation.is_some() {
        return Err(AppError::openai_invalid(
            "conversation",
            "previous_response_id and conversation cannot be used together",
        ));
    }
    if conversation.is_some() {
        return Err(AppError::openai_unsupported(
            "conversation",
            "conversation is not supported yet",
        ));
    }
    let max_tokens = value.get("max_output_tokens").and_then(Value::as_i64);
    let max_completion_tokens = max_tokens;
    let instructions = value
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_string);
    let store = decode_optional_bool(value.get("store"), "store", true)?;
    let reasoning = value.get("reasoning").cloned();
    let tool_choice = value.get("tool_choice").cloned();
    let parallel_tool_calls = value.get("parallel_tool_calls").and_then(Value::as_bool);
    let text = value.get("text").cloned();
    let reasoning_requested = reasoning.is_some();
    let tool_choice_requested = tool_choice.is_some();
    let structured_output_requested = responses_structured_output_requested(value);
    let streaming_usage_requested = value
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let input = value.get("input").unwrap_or(&Value::Null);

    let request = InternalRequest {
        model,
        stream,
        max_tokens,
        max_completion_tokens,
        previous_response_id,
        instructions,
        store,
        reasoning,
        tool_choice,
        parallel_tool_calls,
        text,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested,
        parallel_tool_calls_requested: parallel_tool_calls.unwrap_or(false),
        streaming_usage_requested,
        tools: decode_tools(value.get("tools"))?,
        messages: decode_input(input)?,
    };
    Ok(request)
}

fn decode_optional_bool(
    value: Option<&Value>,
    param: &'static str,
    default: bool,
) -> Result<bool, AppError> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(AppError::openai_invalid(
            param,
            format!("{param} must be a boolean or null"),
        )),
    }
}

fn decode_previous_response_id(value: Option<&Value>) -> Result<Option<String>, AppError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if !id.trim().is_empty() => Ok(Some(id.clone())),
        Some(Value::String(_)) => Err(AppError::openai_invalid(
            "previous_response_id",
            "previous_response_id must not be empty",
        )),
        Some(_) => Err(AppError::openai_invalid(
            "previous_response_id",
            "previous_response_id must be a string or null",
        )),
    }
}

fn decode_conversation(value: Option<&Value>) -> Result<Option<String>, AppError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if !id.trim().is_empty() => Ok(Some(id.clone())),
        Some(Value::Object(object)) => object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .map(Some)
            .ok_or_else(|| {
                AppError::openai_invalid(
                    "conversation",
                    "conversation must be a string or an object with a non-empty id",
                )
            }),
        Some(_) => Err(AppError::openai_invalid(
            "conversation",
            "conversation must be a string or an object with a non-empty id",
        )),
    }
}

/// Validate the subset of Responses requests that can be represented by the
/// internal chat bridge without silently changing their meaning.
pub fn validate_responses_bridge_payload(value: &Value) -> Result<(), AppError> {
    if value
        .get("instructions")
        .is_some_and(|instructions| !instructions.is_null() && !instructions.is_string())
    {
        return Err(unsupported_bridge_feature("instruction items"));
    }
    if value.get("reasoning").is_some() {
        return Err(unsupported_bridge_feature("reasoning"));
    }
    if responses_structured_output_requested(value) {
        return Err(unsupported_bridge_feature("structured output"));
    }
    if let Some(tools) = value.get("tools").and_then(Value::as_array) {
        for tool in tools {
            let tool_type = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("function");
            if tool_type != "function" {
                return Err(unsupported_bridge_feature(format!(
                    "tool type `{tool_type}`"
                )));
            }
        }
    }

    let Some(input) = value.get("input") else {
        return Ok(());
    };
    if let Some(items) = input.as_array() {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str);
            match item_type {
                Some("message" | "function_call" | "function_call_output") | None => {}
                Some(other) => {
                    return Err(unsupported_bridge_feature(format!(
                        "input item type `{other}`"
                    )));
                }
            }

            if let Some(parts) = item.get("content").and_then(Value::as_array) {
                for part in parts {
                    let part_type = part.get("type").and_then(Value::as_str).unwrap_or("text");
                    if !matches!(part_type, "input_text" | "output_text" | "text" | "refusal") {
                        return Err(unsupported_bridge_feature(format!(
                            "content part type `{part_type}`"
                        )));
                    }
                }
            }
            if let Some(parts) = item.get("output").and_then(Value::as_array) {
                for part in parts {
                    let part_type = part.get("type").and_then(Value::as_str).unwrap_or("text");
                    if !matches!(part_type, "input_text" | "text") {
                        return Err(unsupported_bridge_feature(format!(
                            "function output part type `{part_type}`"
                        )));
                    }
                }
            }
        }
    } else if !input.is_null() && !input.is_string() {
        return Err(unsupported_bridge_feature("non-text input value"));
    }

    Ok(())
}

/// Bridge 路径只能承载文本历史；`previous_response_id` 加载的图片必须在
/// 进入 Chat 编码前被明确拒绝，不能静默丢弃或伪造成文本。
pub fn validate_internal_request_for_bridge(request: &InternalRequest) -> Result<(), AppError> {
    if request
        .messages
        .iter()
        .any(|message| message.content.iter().any(InternalContentPart::is_image))
    {
        return Err(unsupported_bridge_feature("input images"));
    }
    Ok(())
}

fn unsupported_bridge_feature(feature: impl Into<String>) -> AppError {
    AppError::BadRequest(format!(
        "Responses bridge does not support {}",
        feature.into()
    ))
}

fn responses_structured_output_requested(value: &Value) -> bool {
    value
        .pointer("/text/format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text")
        || value
            .pointer("/response_format/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "text")
}

fn decode_tools(value: Option<&Value>) -> Result<Vec<InternalTool>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(tools) = value.as_array() else {
        return Ok(Vec::new());
    };

    Ok(tools.iter().filter_map(decode_tool).collect())
}

fn decode_tool(tool: &Value) -> Option<InternalTool> {
    let function = tool.get("function").unwrap_or(tool);
    let name = function
        .get("name")
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())?;
    let name = name.to_string();
    let description = function
        .get("description")
        .or_else(|| tool.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let input_schema = function
        .get("parameters")
        .or_else(|| function.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "object" }));

    Some(InternalTool {
        name,
        description,
        input_schema,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
