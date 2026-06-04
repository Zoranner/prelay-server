#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalRequest {
    pub model: String,
    pub stream: bool,
    pub previous_response_id: Option<String>,
    pub messages: Vec<InternalMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalResponse {
    pub id: String,
    pub model: String,
    pub output: Vec<InternalOutputItem>,
    pub usage: Option<InternalUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalOutputItem {
    Message {
        id: String,
        role: InternalRole,
        content: Vec<InternalContentPart>,
    },
}

#[cfg(test)]
impl InternalOutputItem {
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalMessage {
    pub role: InternalRole,
    pub content: Vec<InternalContentPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalContentPart {
    Text(String),
}
