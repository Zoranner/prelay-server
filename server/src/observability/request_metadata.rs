use serde::Serialize;
use serde_json::Value;

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

#[derive(Debug, Clone, Default)]
pub struct StreamMetadataUpdate {
    pub empty: Option<bool>,
    pub completed: Option<bool>,
    pub final_usage_seen: Option<bool>,
    pub stream_error: Option<String>,
    pub upstream_request_id: Option<String>,
    pub upstream_error_body_excerpt: Option<String>,
}

pub fn update_stream_metadata(
    metadata_json: Option<&str>,
    update: &StreamMetadataUpdate,
) -> Result<String, AppError> {
    let mut metadata = match metadata_json.filter(|metadata| !metadata.trim().is_empty()) {
        Some(raw_metadata) => match serde_json::from_str::<Value>(raw_metadata) {
            Ok(metadata) => metadata,
            Err(error) => serde_json::json!({
                "schema": "provider-relay.request_metadata.v1",
                "bridge": {},
                "diagnostics": [],
                "stream": {},
                "upstream": {},
                "metadata_parse_error": {
                    "raw": raw_metadata,
                    "message": error.to_string()
                }
            }),
        },
        None => serde_json::json!({
            "schema": "provider-relay.request_metadata.v1",
            "bridge": {},
            "diagnostics": [],
            "stream": {},
            "upstream": {}
        }),
    };

    if !metadata.is_object() {
        metadata = serde_json::json!({
            "schema": "provider-relay.request_metadata.v1",
            "bridge": {},
            "diagnostics": [],
            "stream": {},
            "upstream": {}
        });
    }

    ensure_object_field(&mut metadata, "stream");
    ensure_object_field(&mut metadata, "upstream");

    if let Some(stream) = metadata.get_mut("stream").and_then(Value::as_object_mut) {
        if let Some(empty) = update.empty {
            stream.insert("empty".to_string(), Value::Bool(empty));
        }
        if let Some(completed) = update.completed {
            stream.insert("completed".to_string(), Value::Bool(completed));
        }
        if let Some(final_usage_seen) = update.final_usage_seen {
            stream.insert(
                "final_usage_seen".to_string(),
                Value::Bool(final_usage_seen),
            );
        }
        if let Some(stream_error) = &update.stream_error {
            stream.insert(
                "stream_error".to_string(),
                Value::String(stream_error.clone()),
            );
        }
    }

    if let Some(upstream) = metadata.get_mut("upstream").and_then(Value::as_object_mut) {
        if let Some(request_id) = &update.upstream_request_id {
            upstream.insert("request_id".to_string(), Value::String(request_id.clone()));
        }
        if let Some(error_body_excerpt) = &update.upstream_error_body_excerpt {
            upstream.insert(
                "error_body_excerpt".to_string(),
                Value::String(error_body_excerpt.clone()),
            );
        }
    }

    serde_json::to_string(&metadata).map_err(|error| AppError::Internal(error.into()))
}

fn ensure_object_field(metadata: &mut Value, field: &str) {
    if !metadata.get(field).is_some_and(Value::is_object) {
        metadata[field] = serde_json::json!({});
    }
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
        observability::request_metadata::{
            build_request_metadata, update_stream_metadata, StreamMetadataUpdate,
        },
        providers::spec::UpstreamProtocol,
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

    #[test]
    fn updates_stream_metadata_without_losing_bridge_fields() {
        let metadata = build_request_metadata(
            "responses",
            "responses",
            UpstreamProtocol::ChatCompletions,
            "coder",
            "deepseek-chat",
            Vec::new(),
        )
        .expect("serialize request metadata");

        let updated = update_stream_metadata(
            Some(&metadata),
            &StreamMetadataUpdate {
                empty: Some(false),
                completed: Some(true),
                final_usage_seen: Some(true),
                stream_error: None,
                upstream_request_id: Some("req_123".to_string()),
                upstream_error_body_excerpt: None,
            },
        )
        .expect("update metadata");
        let metadata: serde_json::Value =
            serde_json::from_str(&updated).expect("parse updated metadata");

        assert_eq!(metadata["bridge"]["model_requested"], "coder");
        assert_eq!(metadata["stream"]["empty"], false);
        assert_eq!(metadata["stream"]["completed"], true);
        assert_eq!(metadata["stream"]["final_usage_seen"], true);
        assert_eq!(metadata["upstream"]["request_id"], "req_123");
    }

    #[test]
    fn updates_stream_metadata_preserves_invalid_metadata_for_audit() {
        let updated = update_stream_metadata(
            Some("{invalid metadata"),
            &StreamMetadataUpdate {
                empty: Some(false),
                completed: Some(true),
                final_usage_seen: Some(false),
                stream_error: None,
                upstream_request_id: Some("req_bad_metadata".to_string()),
                upstream_error_body_excerpt: None,
            },
        )
        .expect("update invalid metadata");
        let metadata: serde_json::Value =
            serde_json::from_str(&updated).expect("parse updated metadata");

        assert_eq!(metadata["stream"]["empty"], false);
        assert_eq!(metadata["stream"]["completed"], true);
        assert_eq!(metadata["stream"]["final_usage_seen"], false);
        assert_eq!(metadata["upstream"]["request_id"], "req_bad_metadata");
        assert_eq!(metadata["metadata_parse_error"]["raw"], "{invalid metadata");
        assert!(metadata["metadata_parse_error"]["message"].is_string());
    }
}
