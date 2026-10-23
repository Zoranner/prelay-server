use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};

use crate::bridge::internal::{
    InternalContentPart, InternalImagePart, InternalMessage, InternalRequest, InternalRole,
};

pub fn encode_responses_request(request: &InternalRequest) -> Value {
    let mut value = json!({
        "model": request.model,
        "stream": request.stream,
        "input": request
            .messages
            .iter()
            .flat_map(encode_message_items)
            .collect::<Vec<_>>(),
        "store": request.store,
    });
    if let Some(instructions) = &request.instructions {
        value["instructions"] = json!(instructions);
    }
    if let Some(reasoning) = &request.reasoning {
        value["reasoning"] = reasoning.clone();
    }
    if let Some(tool_choice) = &request.tool_choice {
        value["tool_choice"] = tool_choice.clone();
    }
    if let Some(parallel_tool_calls) = request.parallel_tool_calls {
        value["parallel_tool_calls"] = json!(parallel_tool_calls);
    }
    if let Some(text) = &request.text {
        value["text"] = text.clone();
    }
    if let Some(max_tokens) = request.max_completion_tokens.or(request.max_tokens) {
        value["max_output_tokens"] = json!(max_tokens);
    }
    if !request.tools.is_empty() {
        value["tools"] = json!(request.tools.iter().map(encode_tool).collect::<Vec<_>>());
    }
    value
}

/// 原生 Responses 直通的上行请求体：按原 payload 写出，只替换顶层 `model`。
///
/// 直通时 payload 可能就是几十 MB（长会话、截图），为换一个模型名深拷贝整个
/// `Value` 代价过高；这里直接序列化写出，结果与“先替换 `model` 再序列化”一致。
pub fn encode_native_responses_request_body(payload: &Value, model: &str) -> Vec<u8> {
    let mut body = Vec::new();
    serde_json::to_writer(&mut body, &PayloadWithModel { payload, model })
        .expect("serializing a JSON payload into a Vec cannot fail");
    body
}

struct PayloadWithModel<'a> {
    payload: &'a Value,
    model: &'a str,
}

impl Serialize for PayloadWithModel<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Some(object) = self.payload.as_object() else {
            return self.payload.serialize(serializer);
        };

        let mut map = serializer.serialize_map(Some(object.len().max(1)))?;
        let mut replaced = false;
        for (key, value) in object {
            if key == "model" {
                map.serialize_entry(key, self.model)?;
                replaced = true;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        if !replaced {
            map.serialize_entry("model", self.model)?;
        }
        map.end()
    }
}

fn encode_message_items(message: &InternalMessage) -> Vec<Value> {
    if matches!(message.role, InternalRole::Tool) {
        return vec![json!({
            "type": "function_call_output",
            "call_id": message.tool_call_id,
            "output": encode_output_content(&message.content),
        })];
    }

    let mut items = Vec::new();
    if !message.content.is_empty() || message.tool_calls.is_empty() {
        items.push(json!({
            "role": encode_role(&message.role),
            "content": encode_message_content(&message.role, &message.content),
        }));
    }
    for tool_call in &message.tool_calls {
        items.push(json!({
            "type": "function_call",
            "call_id": tool_call.id,
            "name": tool_call.name,
            "arguments": tool_call.arguments,
        }));
    }
    items
}

fn encode_message_content(role: &InternalRole, content: &[InternalContentPart]) -> Value {
    if content
        .iter()
        .all(|part| matches!(part, InternalContentPart::Text(_)))
    {
        return Value::String(join_text_content(content));
    }

    let text_type = match role {
        InternalRole::Assistant => "output_text",
        _ => "input_text",
    };
    Value::Array(
        content
            .iter()
            .map(|part| match part {
                InternalContentPart::Text(text) => json!({
                    "type": text_type,
                    "text": text,
                }),
                InternalContentPart::Image(image) => encode_image_part(image),
            })
            .collect(),
    )
}

fn encode_output_content(content: &[InternalContentPart]) -> Value {
    if content
        .iter()
        .all(|part| matches!(part, InternalContentPart::Text(_)))
    {
        return Value::String(join_text_content(content));
    }

    Value::Array(
        content
            .iter()
            .map(|part| match part {
                InternalContentPart::Text(text) => json!({
                    "type": "input_text",
                    "text": text,
                }),
                InternalContentPart::Image(image) => encode_image_part(image),
            })
            .collect(),
    )
}

fn encode_image_part(image: &InternalImagePart) -> Value {
    let mut part = json!({ "type": "input_image" });
    if let Some(image_url) = &image.image_url {
        part["image_url"] = image_url.clone();
    }
    if let Some(file_id) = &image.file_id {
        part["file_id"] = json!(file_id);
    }
    if let Some(detail) = &image.detail {
        part["detail"] = json!(detail);
    }
    part
}

fn encode_tool(tool: &crate::bridge::internal::InternalTool) -> Value {
    let mut value = json!({
        "type": "function",
        "name": tool.name,
        "parameters": tool.input_schema,
    });
    if let Some(description) = &tool.description {
        value["description"] = json!(description);
    }
    value
}

fn encode_role(role: &InternalRole) -> &'static str {
    match role {
        InternalRole::User => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
        InternalRole::Tool => "tool",
    }
}

fn join_text_content(content: &[InternalContentPart]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            InternalContentPart::Text(text) => Some(text.as_str()),
            InternalContentPart::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
