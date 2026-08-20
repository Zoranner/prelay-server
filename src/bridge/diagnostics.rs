use serde::{Deserialize, Serialize};

use crate::bridge::internal::InternalRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedRequest {
    pub request: InternalRequest,
    pub diagnostics: Vec<BridgeDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeDiagnostic {
    pub phase: DiagnosticPhase,
    pub protocol: String,
    pub path: String,
    pub action: DiagnosticAction,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub original_kind: Option<String>,
}

impl BridgeDiagnostic {
    pub fn new(
        protocol: impl Into<String>,
        path: impl Into<String>,
        action: DiagnosticAction,
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
        original_kind: Option<String>,
    ) -> Self {
        Self {
            phase: DiagnosticPhase::Decode,
            protocol: protocol.into(),
            path: path.into(),
            action,
            severity,
            code: code.into(),
            message: message.into(),
            original_kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Decode,
    Encode,
    StreamDecode,
    StreamEncode,
    Upstream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticAction {
    Mapped,
    Defaulted,
    Ignored,
    Textified,
    PassedThrough,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
}
