use serde_json::{json, Value};

use crate::bridge::internal::InternalContentPart;

pub(super) fn internal_content_value(content: &[InternalContentPart]) -> Option<Value> {
    let parts = content
        .iter()
        .map(|part| match part {
            InternalContentPart::Text(text) => Value::String(text.clone()),
            InternalContentPart::Image(image) => {
                let mut value = json!({
                    "type": "input_image",
                    "omitted": "media",
                });
                if let Some(detail) = &image.detail {
                    value["detail"] = Value::String(detail.clone());
                }
                if let Some(file_id) = &image.file_id {
                    value["file_id"] = Value::String(file_id.clone());
                }
                value
            }
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else if parts.len() == 1 && parts[0].is_string() {
        parts.into_iter().next()
    } else {
        Some(Value::Array(parts))
    }
}

pub(super) fn chat_content_value(content: &Value) -> Option<Value> {
    match content {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::String(text) => Some(Value::String(text.clone())),
        Value::Array(items) => sanitized_content_parts(items),
        _ => None,
    }
}

fn sanitized_content_parts(items: &[Value]) -> Option<Value> {
    let parts = items
        .iter()
        .filter_map(sanitized_content_part)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(Value::Array(parts))
    }
}

fn sanitized_content_part(part: &Value) -> Option<Value> {
    let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
    if let Some(text) = part
        .get("text")
        .or_else(|| part.get("input_text"))
        .and_then(Value::as_str)
    {
        return Some(json!({
            "type": if part_type.is_empty() { "text" } else { part_type },
            "text": text,
        }));
    }
    match part_type {
        "image" | "image_url" | "input_image" | "file" | "input_file" | "audio" => Some(json!({
            "type": part_type,
            "omitted": "media",
        })),
        "" => None,
        _ => Some(json!({
            "type": part_type,
            "omitted": "non_text",
        })),
    }
}
