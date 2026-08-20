use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;

use crate::bridge::internal::{
    InternalMessage, InternalOutputItem, InternalResponse, InternalToolCall,
};

pub struct ResponseSessionInsert<'a> {
    pub identity_id: &'a str,
    pub response_id: &'a str,
    pub previous_response_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub input_messages: &'a [InternalMessage],
    pub response: &'a InternalResponse,
}

pub async fn save_response_session(
    pool: &SqlitePool,
    insert: ResponseSessionInsert<'_>,
) -> Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO identity_response_sessions (response_id, identity_id, \
         previous_response_id, provider_id, model, input_messages_json, output_items_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(insert.response_id)
    .bind(insert.identity_id)
    .bind(insert.previous_response_id)
    .bind(insert.provider_id)
    .bind(insert.model)
    .bind(serde_json::to_string(insert.input_messages)?)
    .bind(serde_json::to_string(&insert.response.output)?)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_response_session_messages(
    pool: &SqlitePool,
    identity_id: &str,
    response_id: &str,
) -> Result<Option<Vec<InternalMessage>>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        input_messages_json: String,
        output_items_json: String,
    }
    let row = sqlx::query_as::<_, Row>(
        "SELECT input_messages_json, output_items_json FROM identity_response_sessions \
         WHERE response_id = ? AND identity_id = ?",
    )
    .bind(response_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut messages: Vec<InternalMessage> = serde_json::from_str(&row.input_messages_json)?;
    let items: Vec<InternalOutputItem> = serde_json::from_str(&row.output_items_json)?;
    messages.extend(items.into_iter().map(output_item_to_message));
    Ok(Some(messages))
}

fn output_item_to_message(item: InternalOutputItem) -> InternalMessage {
    match item {
        InternalOutputItem::Message { role, content, .. } => InternalMessage {
            role,
            content,
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        },
        InternalOutputItem::FunctionToolCall {
            id,
            name,
            arguments,
            reasoning_content,
        } => InternalMessage {
            role: crate::bridge::internal::InternalRole::Assistant,
            content: Vec::new(),
            tool_call_id: None,
            tool_calls: vec![InternalToolCall {
                id,
                name,
                arguments,
            }],
            reasoning_content,
        },
    }
}
