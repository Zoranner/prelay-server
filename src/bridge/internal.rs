use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalRequest {
    pub model: String,
    pub stream: bool,
    pub max_tokens: Option<i64>,
    pub previous_response_id: Option<String>,
    pub reasoning_requested: bool,
    pub tool_choice_requested: bool,
    pub structured_output_requested: bool,
    pub tools: Vec<InternalTool>,
    pub messages: Vec<InternalMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
}

impl InternalOutputItem {
    pub fn is_tool_call(&self) -> bool {
        matches!(self, InternalOutputItem::FunctionToolCall { .. })
    }

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

    #[cfg(test)]
    pub fn tool_call_reasoning_content(&self) -> Option<String> {
        match self {
            InternalOutputItem::FunctionToolCall {
                reasoning_content, ..
            } => reasoning_content.clone(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
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
