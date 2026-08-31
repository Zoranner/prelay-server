mod config;
mod content;
mod redaction;
mod stream;

use crate::{
    bridge::internal::{InternalOutputItem, InternalRequest, InternalResponse},
    stats::ActivityInsert,
    storage::{Storage, StorageError},
};
use serde_json::Value;

pub use config::{initialize_from_environment, policy, ActivityContentPolicy};
pub use content::media_metadata_from_bytes;
pub use content::{
    activity_content_from_text, activity_content_from_text_with_media, ActivityContentDraft,
    ActivityMediaMetadata, NormalizedActivityContent,
};
pub use stream::{RawStreamContentCapture, RawStreamProtocol};

pub const DEFAULT_ACTIVITY_CONTENT_MAX_BYTES: usize = 64 * 1024;

pub fn internal_request_text(request: &InternalRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|part| match part {
            crate::bridge::internal::InternalContentPart::Text(text) => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn internal_response_text(response: &InternalResponse) -> String {
    response
        .output
        .iter()
        .filter_map(|output| match output {
            InternalOutputItem::Message { content, .. } => Some(
                content
                    .iter()
                    .map(|part| match part {
                        crate::bridge::internal::InternalContentPart::Text(text) => text.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            InternalOutputItem::FunctionToolCall { .. } => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn chat_message_text(payload: &Value) -> String {
    payload
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| message.get("content"))
        .map(chat_content_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn chat_response_text(payload: &Value) -> String {
    payload
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| choice.pointer("/message/content"))
        .map(chat_content_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn anthropic_message_text(payload: &Value) -> String {
    payload
        .get("content")
        .map(anthropic_content_text)
        .unwrap_or_default()
}

pub fn anthropic_request_text(payload: &Value) -> String {
    let system_text = payload
        .get("system")
        .map(anthropic_content_text)
        .unwrap_or_default();
    let message_text = payload
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(anthropic_message_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    match (system_text.is_empty(), message_text.is_empty()) {
        (true, _) => message_text,
        (_, true) => system_text,
        (false, false) => format!("{system_text}\n{message_text}"),
    }
}

fn chat_content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| {
                matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("text" | "input_text")
                )
            })
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("input_text"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn anthropic_content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub async fn enqueue_activity_content_best_effort(
    storage: &Storage,
    activity_id: String,
    input_text: &str,
    output_text: &str,
    media: Option<ActivityMediaMetadata>,
) {
    enqueue_activity_content_with_capture_best_effort(
        storage,
        activity_id,
        input_text,
        output_text,
        media,
        false,
    )
    .await;
}

pub async fn enqueue_activity_content_with_capture_best_effort(
    storage: &Storage,
    activity_id: String,
    input_text: &str,
    output_text: &str,
    media: Option<ActivityMediaMetadata>,
    capture_truncated: bool,
) {
    let Some(mut content) =
        activity_content_from_text_with_media(input_text, output_text, media, policy().max_bytes)
    else {
        return;
    };
    content.is_truncated |= capture_truncated;

    if storage
        .enqueue_activity_content(content.into_draft(activity_id))
        .await
        .is_err()
    {
        tracing::warn!(
            failure_kind = "activity_content_storage",
            "failed to persist activity content"
        );
    }
}

pub async fn insert_activity_with_content(
    storage: &Storage,
    identity_id: &str,
    activity: ActivityInsert,
    input_text: &str,
    output_text: &str,
    media: Option<ActivityMediaMetadata>,
) -> Result<(), StorageError> {
    let activity_id = storage.insert_activity(identity_id, activity).await?;
    enqueue_activity_content_best_effort(storage, activity_id, input_text, output_text, media)
        .await;
    Ok(())
}

#[cfg(test)]
mod tests;
