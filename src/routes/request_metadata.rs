use serde::Serialize;

use crate::bridge::diagnostics::BridgeDiagnostic;
use crate::error::AppError;
use crate::providers::spec::UpstreamProtocol;

#[derive(Debug, Clone, Default)]
pub struct RequestMetadataBuilder {
    protocol_in: Option<String>,
    protocol_out: Option<String>,
    protocol_upstream: Option<String>,
    model_requested: Option<String>,
    model_upstream: Option<String>,
    diagnostics: Vec<BridgeDiagnostic>,
}

pub fn build_request_metadata(
    protocol_in: &str,
    protocol_out: &str,
    protocol_upstream: UpstreamProtocol,
    model_requested: &str,
    model_upstream: &str,
    diagnostics: Vec<BridgeDiagnostic>,
) -> Result<String, AppError> {
    RequestMetadataBuilder::new()
        .protocol_in(protocol_in)
        .protocol_out(protocol_out)
        .protocol_upstream(upstream_protocol_label(protocol_upstream))
        .model_requested(model_requested)
        .model_upstream(model_upstream)
        .diagnostics(diagnostics)
        .into_json_string()
        .map_err(|error| AppError::Internal(error.into()))
}

fn upstream_protocol_label(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::ChatCompletions => "chat_completions",
        UpstreamProtocol::AnthropicMessages => "anthropic_messages",
    }
}

impl RequestMetadataBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn protocol_in(mut self, value: impl Into<String>) -> Self {
        self.protocol_in = Some(value.into());
        self
    }

    pub fn protocol_out(mut self, value: impl Into<String>) -> Self {
        self.protocol_out = Some(value.into());
        self
    }

    pub fn protocol_upstream(mut self, value: impl Into<String>) -> Self {
        self.protocol_upstream = Some(value.into());
        self
    }

    pub fn model_requested(mut self, value: impl Into<String>) -> Self {
        self.model_requested = Some(value.into());
        self
    }

    pub fn model_upstream(mut self, value: impl Into<String>) -> Self {
        self.model_upstream = Some(value.into());
        self
    }

    pub fn diagnostics(mut self, diagnostics: Vec<BridgeDiagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn into_json_string(self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&RequestMetadata {
            schema: "provider-relay.request_metadata.v1",
            bridge: BridgeMetadata {
                protocol_in: self.protocol_in,
                protocol_out: self.protocol_out,
                protocol_upstream: self.protocol_upstream,
                model_requested: self.model_requested,
                model_upstream: self.model_upstream,
            },
            diagnostics: self.diagnostics,
            stream: StreamMetadata {
                empty: None,
                completed: None,
                final_usage_seen: None,
                stream_error: None,
            },
            upstream: UpstreamMetadata {
                request_id: None,
                error_body_excerpt: None,
            },
        })
    }
}

#[derive(Debug, Serialize)]
struct RequestMetadata {
    schema: &'static str,
    bridge: BridgeMetadata,
    diagnostics: Vec<BridgeDiagnostic>,
    stream: StreamMetadata,
    upstream: UpstreamMetadata,
}

#[derive(Debug, Serialize)]
struct BridgeMetadata {
    protocol_in: Option<String>,
    protocol_out: Option<String>,
    protocol_upstream: Option<String>,
    model_requested: Option<String>,
    model_upstream: Option<String>,
}

#[derive(Debug, Serialize)]
struct StreamMetadata {
    empty: Option<bool>,
    completed: Option<bool>,
    final_usage_seen: Option<bool>,
    stream_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpstreamMetadata {
    request_id: Option<String>,
    error_body_excerpt: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::{
        bridge::diagnostics::{BridgeDiagnostic, DiagnosticAction, DiagnosticSeverity},
        providers::spec::UpstreamProtocol,
        routes::request_metadata::build_request_metadata,
    };

    #[test]
    fn builds_request_metadata_without_user_content() {
        let metadata = build_request_metadata(
            "responses",
            "responses",
            UpstreamProtocol::ChatCompletions,
            "coder",
            "deepseek-chat",
            vec![BridgeDiagnostic::new(
                "responses",
                "/input/0/role",
                DiagnosticAction::Mapped,
                DiagnosticSeverity::Warning,
                "responses.role.unknown",
                "未知 role 已映射为 user",
                Some("planner".to_string()),
            )],
        )
        .expect("serialize request metadata");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).expect("parse metadata"),
            serde_json::json!({
                "schema": "provider-relay.request_metadata.v1",
                "bridge": {
                    "protocol_in": "responses",
                    "protocol_out": "responses",
                    "protocol_upstream": "chat_completions",
                    "model_requested": "coder",
                    "model_upstream": "deepseek-chat"
                },
                "diagnostics": [
                    {
                        "phase": "decode",
                        "protocol": "responses",
                        "path": "/input/0/role",
                        "action": "mapped",
                        "severity": "warning",
                        "code": "responses.role.unknown",
                        "message": "未知 role 已映射为 user",
                        "original_kind": "planner"
                    }
                ],
                "stream": {
                    "empty": null,
                    "completed": null,
                    "final_usage_seen": null,
                    "stream_error": null
                },
                "upstream": {
                    "request_id": null,
                    "error_body_excerpt": null
                }
            })
        );
    }
}
