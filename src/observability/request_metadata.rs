use serde::{Deserialize, Serialize};

use crate::bridge::diagnostics::BridgeDiagnostic;
use crate::error::AppError;

const METADATA_SCHEMA: &str = "provider-relay.request_metadata.v2";
const DIAGNOSTIC_PATH_SAMPLE_LIMIT: usize = 3;

pub fn build_request_metadata(
    diagnostics: Vec<BridgeDiagnostic>,
) -> Result<Option<String>, AppError> {
    let diagnostics = compact_diagnostics(diagnostics);
    if diagnostics.is_empty() {
        return Ok(None);
    }

    serialize_metadata(RequestMetadata {
        diagnostics,
        ..RequestMetadata::new()
    })
}

#[derive(Debug, Clone, Default)]
pub struct StreamMetadataUpdate {
    pub empty: Option<bool>,
    pub completed: Option<bool>,
    pub final_usage_seen: Option<bool>,
}

pub fn update_stream_metadata(
    metadata_json: Option<&str>,
    update: &StreamMetadataUpdate,
) -> Result<Option<String>, AppError> {
    let mut metadata = match metadata_json.filter(|metadata| !metadata.trim().is_empty()) {
        Some(raw_metadata) => match serde_json::from_str(raw_metadata) {
            Ok(metadata) => metadata,
            Err(_) => RequestMetadata {
                metadata_parse_error: true,
                ..RequestMetadata::new()
            },
        },
        None => RequestMetadata::new(),
    };

    let stream_is_anomalous = update.empty == Some(true)
        || update.completed == Some(false)
        || update.final_usage_seen == Some(false);
    if !stream_is_anomalous && metadata.diagnostics.is_empty() && !metadata.metadata_parse_error {
        return Ok(None);
    }

    if stream_is_anomalous {
        metadata.stream = Some(StreamMetadata {
            empty: update.empty,
            completed: update.completed,
            final_usage_seen: update.final_usage_seen,
        });
    }

    serialize_metadata(metadata)
}

fn compact_diagnostics(diagnostics: Vec<BridgeDiagnostic>) -> Vec<DiagnosticMetadata> {
    let mut summaries = Vec::<DiagnosticMetadata>::new();
    for diagnostic in diagnostics {
        if let Some(summary) = summaries.iter_mut().find(|summary| {
            summary.phase == diagnostic.phase
                && summary.protocol == diagnostic.protocol
                && summary.action == diagnostic.action
                && summary.severity == diagnostic.severity
                && summary.code == diagnostic.code
                && summary.message == diagnostic.message
                && summary.original_kind == diagnostic.original_kind
        }) {
            summary.count += 1;
            if summary.paths.len() < DIAGNOSTIC_PATH_SAMPLE_LIMIT {
                summary.paths.push(diagnostic.path);
            }
            continue;
        }

        summaries.push(DiagnosticMetadata {
            phase: diagnostic.phase,
            protocol: diagnostic.protocol,
            action: diagnostic.action,
            severity: diagnostic.severity,
            code: diagnostic.code,
            message: diagnostic.message,
            original_kind: diagnostic.original_kind,
            count: 1,
            paths: vec![diagnostic.path],
        });
    }
    summaries
}

fn serialize_metadata(metadata: RequestMetadata) -> Result<Option<String>, AppError> {
    serde_json::to_string(&metadata)
        .map(Some)
        .map_err(|error| AppError::Internal(error.into()))
}

#[derive(Debug, Deserialize, Serialize)]
struct RequestMetadata {
    schema: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<DiagnosticMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream: Option<StreamMetadata>,
    #[serde(default, skip_serializing_if = "is_false")]
    metadata_parse_error: bool,
}

impl RequestMetadata {
    fn new() -> Self {
        Self {
            schema: METADATA_SCHEMA.to_string(),
            diagnostics: Vec::new(),
            stream: None,
            metadata_parse_error: false,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Deserialize, Serialize)]
struct StreamMetadata {
    empty: Option<bool>,
    completed: Option<bool>,
    final_usage_seen: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiagnosticMetadata {
    phase: crate::bridge::diagnostics::DiagnosticPhase,
    protocol: String,
    action: crate::bridge::diagnostics::DiagnosticAction,
    severity: crate::bridge::diagnostics::DiagnosticSeverity,
    code: String,
    message: String,
    original_kind: Option<String>,
    count: usize,
    paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use crate::{
        bridge::diagnostics::{BridgeDiagnostic, DiagnosticAction, DiagnosticSeverity},
        observability::request_metadata::{
            build_request_metadata, update_stream_metadata, StreamMetadataUpdate,
        },
    };

    #[test]
    fn omits_metadata_without_diagnostics() {
        let metadata = build_request_metadata(Vec::new()).expect("serialize request metadata");

        assert!(metadata.is_none());
    }

    #[test]
    fn aggregates_repeated_diagnostics_and_limits_path_samples() {
        let diagnostic = |path| {
            BridgeDiagnostic::new(
                "responses",
                path,
                DiagnosticAction::Textified,
                DiagnosticSeverity::Info,
                "responses.content.non_text",
                "非文本 content 已转为 JSON 字符串",
                Some("object".to_string()),
            )
        };
        let metadata = build_request_metadata(vec![
            diagnostic("/input/0/content"),
            diagnostic("/input/1/content"),
            diagnostic("/input/2/content"),
            diagnostic("/input/3/content"),
        ])
        .expect("serialize request metadata")
        .expect("metadata for diagnostics");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).expect("parse metadata"),
            serde_json::json!({
                "schema": "provider-relay.request_metadata.v2",
                "diagnostics": [{
                    "phase": "decode",
                    "protocol": "responses",
                    "action": "textified",
                    "severity": "info",
                    "code": "responses.content.non_text",
                    "message": "非文本 content 已转为 JSON 字符串",
                    "original_kind": "object",
                    "count": 4,
                    "paths": [
                        "/input/0/content",
                        "/input/1/content",
                        "/input/2/content"
                    ]
                }]
            })
        );
    }

    #[test]
    fn omits_metadata_for_completed_stream_without_diagnostics() {
        let updated = update_stream_metadata(
            None,
            &StreamMetadataUpdate {
                empty: Some(false),
                completed: Some(true),
                final_usage_seen: Some(true),
            },
        )
        .expect("update metadata");

        assert!(updated.is_none());
    }

    #[test]
    fn updates_stream_metadata_preserves_invalid_metadata_for_audit() {
        let updated = update_stream_metadata(
            Some("{invalid metadata"),
            &StreamMetadataUpdate {
                empty: Some(false),
                completed: Some(true),
                final_usage_seen: Some(false),
            },
        )
        .expect("update invalid metadata")
        .expect("metadata for abnormal stream");
        let metadata: serde_json::Value =
            serde_json::from_str(&updated).expect("parse updated metadata");

        assert_eq!(metadata["stream"]["empty"], false);
        assert_eq!(metadata["stream"]["completed"], true);
        assert_eq!(metadata["stream"]["final_usage_seen"], false);
        assert_eq!(metadata["metadata_parse_error"], true);
    }
}
