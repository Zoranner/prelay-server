use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalRequest {
    pub model: String,
    pub stream: bool,
    pub previous_response_id: Option<String>,
    pub messages: Vec<InternalMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalResponse {
    pub id: String,
    pub model: String,
    pub output: Vec<InternalOutputItem>,
    pub usage: Option<InternalUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InternalOutputItem {
    Message {
        id: String,
        role: InternalRole,
        content: Vec<InternalContentPart>,
    },
    FunctionToolCall {
        id: String,
        name: String,
        arguments: String,
    },
}

impl InternalOutputItem {
    #[cfg(test)]
    pub fn text_content(&self) -> Option<String> {
        match self {
            InternalOutputItem::Message { content, .. } => {
                let text = content
                    .iter()
                    .map(|part| match part {
                        InternalContentPart::Text(text) => text.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            InternalOutputItem::FunctionToolCall { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn tool_call_name(&self) -> Option<String> {
        match self {
            InternalOutputItem::FunctionToolCall { name, .. } => Some(name.clone()),
            InternalOutputItem::Message { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn tool_call_arguments(&self) -> Option<String> {
        match self {
            InternalOutputItem::FunctionToolCall { arguments, .. } => Some(arguments.clone()),
            InternalOutputItem::Message { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalMessage {
    pub role: InternalRole,
    pub content: Vec<InternalContentPart>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<InternalToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InternalRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InternalContentPart {
    Text(String),
}
