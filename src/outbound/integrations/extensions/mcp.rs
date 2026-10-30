use prelay_protocol::ExtensionMcpManifest;

use super::CatalogError;

/// 使用协议仓的共享规则校验固定版本 `server.json`，失败统一归为目录内容无效。
pub(super) fn validate_mcp_manifest(manifest: &ExtensionMcpManifest) -> Result<(), CatalogError> {
    prelay_protocol::validate_mcp_manifest(manifest).map_err(|_| CatalogError::ContentInvalid)
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
