use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;

use crate::bridge::internal::{
    InternalMessage, InternalOutputItem, InternalResponse, InternalToolCall,
};

pub async fn save_response_session(
    pool: &SqlitePool,
    response_id: &str,
    previous_response_id: Option<&str>,
    provider_id: &str,
    model: &str,
    input_messages: &[InternalMessage],
    response: &InternalResponse,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO response_sessions (
            response_id,
            previous_response_id,
            provider_id,
            model,
            input_messages_json,
            output_items_json,
            created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(response_id)
    .bind(previous_response_id)
    .bind(provider_id)
    .bind(model)
    .bind(serde_json::to_string(input_messages)?)
    .bind(serde_json::to_string(&response.output)?)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn load_response_session_messages(
    pool: &SqlitePool,
    response_id: &str,
) -> Result<Option<Vec<InternalMessage>>> {
    let row = sqlx::query_as::<_, ResponseSessionRow>(
        r#"
        SELECT input_messages_json, output_items_json
        FROM response_sessions
        WHERE response_id = ?
        "#,
    )
    .bind(response_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    let mut messages: Vec<InternalMessage> = serde_json::from_str(&row.input_messages_json)?;
    let output_items: Vec<InternalOutputItem> = serde_json::from_str(&row.output_items_json)?;
    messages.extend(output_items.into_iter().map(output_item_to_message));

    Ok(Some(messages))
}

#[derive(sqlx::FromRow)]
struct ResponseSessionRow {
    input_messages_json: String,
    output_items_json: String,
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

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{load_response_session_messages, save_response_session};
    use crate::{
        bridge::internal::{
            InternalContentPart, InternalMessage, InternalOutputItem, InternalResponse,
            InternalRole,
        },
        db,
    };

    #[tokio::test]
    async fn saves_and_loads_response_session_messages() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        save_response_session(
            &db,
            "resp_1",
            None,
            "provider-1",
            "deepseek-chat",
            &[InternalMessage {
                role: InternalRole::User,
                content: vec![InternalContentPart::Text("first user".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
            &InternalResponse {
                id: "resp_1".to_string(),
                model: "deepseek-chat".to_string(),
                output: vec![InternalOutputItem::Message {
                    id: "msg_1".to_string(),
                    role: InternalRole::Assistant,
                    content: vec![InternalContentPart::Text("first assistant".to_string())],
                }],
                usage: None,
            },
        )
        .await
        .expect("save session");

        let messages = load_response_session_messages(&db, "resp_1")
            .await
            .expect("load session")
            .expect("session exists");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, InternalRole::User);
        assert_eq!(
            messages[0].content,
            vec![InternalContentPart::Text("first user".to_string())]
        );
        assert_eq!(messages[1].role, InternalRole::Assistant);
        assert_eq!(
            messages[1].content,
            vec![InternalContentPart::Text("first assistant".to_string())]
        );
    }

    #[tokio::test]
    async fn saves_and_loads_tool_call_reasoning_content() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        save_response_session(
            &db,
            "resp_reasoning",
            None,
            "provider-1",
            "deepseek-chat",
            &[InternalMessage {
                role: InternalRole::User,
                content: vec![InternalContentPart::Text("read Cargo.toml".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
            &InternalResponse {
                id: "resp_reasoning".to_string(),
                model: "deepseek-chat".to_string(),
                output: vec![InternalOutputItem::FunctionToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"Cargo.toml\"}".to_string(),
                    reasoning_content: Some("Need to inspect the file first.".to_string()),
                }],
                usage: None,
            },
        )
        .await
        .expect("save session");

        let messages = load_response_session_messages(&db, "resp_reasoning")
            .await
            .expect("load session")
            .expect("session exists");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, InternalRole::Assistant);
        assert_eq!(
            messages[1].reasoning_content.as_deref(),
            Some("Need to inspect the file first.")
        );
        assert_eq!(messages[1].tool_calls[0].id, "call_1");
    }

    #[tokio::test]
    async fn loads_old_session_json_without_reasoning_content() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        sqlx::query(
            r#"
            INSERT INTO response_sessions (
                response_id,
                previous_response_id,
                provider_id,
                model,
                input_messages_json,
                output_items_json,
                created_at
            )
            VALUES (?, NULL, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("resp_old")
        .bind("provider-1")
        .bind("deepseek-chat")
        .bind(
            r#"[{"role":"User","content":[{"Text":"read Cargo.toml"}],"tool_call_id":null,"tool_calls":[]}]"#,
        )
        .bind(
            r#"[{"FunctionToolCall":{"id":"call_1","name":"read_file","arguments":"{\"path\":\"Cargo.toml\"}"}}]"#,
        )
        .bind("2026-01-01T00:00:00Z")
        .execute(&db)
        .await
        .expect("insert old session");

        let messages = load_response_session_messages(&db, "resp_old")
            .await
            .expect("load session")
            .expect("session exists");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].reasoning_content, None);
        assert_eq!(messages[1].reasoning_content, None);
        assert_eq!(messages[1].tool_calls[0].id, "call_1");
    }
}
