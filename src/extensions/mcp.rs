use std::collections::BTreeMap;

use prelay_protocol::{ExtensionMcpManifest, ExtensionMcpTransport};

use super::CatalogError;

pub(super) fn validate_mcp_manifest(manifest: &ExtensionMcpManifest) -> Result<(), CatalogError> {
    if !is_mcp_server_name(&manifest.name) {
        return Err(CatalogError::ContentInvalid);
    }

    match &manifest.transport {
        ExtensionMcpTransport::Stdio {
            command,
            cwd,
            environment,
            enabled,
            ..
        } => {
            if command
                .first()
                .is_none_or(|program| program.trim().is_empty())
                || cwd.is_some()
                || !enabled
                || command_has_plaintext_secret(command)
            {
                return Err(CatalogError::ContentInvalid);
            }
            validate_environment_references(environment)
        }
        ExtensionMcpTransport::Http {
            url,
            headers,
            enabled,
            ..
        } => {
            if !enabled || !is_safe_http_url(url) {
                return Err(CatalogError::ContentInvalid);
            }
            for (name, value) in headers {
                if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err()
                    || !is_environment_variable_name(value)
                {
                    return Err(CatalogError::ContentInvalid);
                }
            }
            Ok(())
        }
    }
}

fn is_mcp_server_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "workspace" | "cli" | "project" | "user" | "local" | "claude" | "builtin"
        )
}

fn is_safe_http_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.query().is_none()
}

fn command_has_plaintext_secret(command: &[String]) -> bool {
    command.iter().skip(1).any(|argument| {
        is_sensitive_command_option(argument) || contains_plaintext_credential(argument)
    })
}

fn is_sensitive_command_option(argument: &str) -> bool {
    let option = argument
        .trim_start_matches('-')
        .split_once('=')
        .map_or(argument.trim_start_matches('-'), |(name, _)| name);
    is_sensitive_name(option)
}

/// 位置参数里直接写成 URL 时，内嵌用户名密码或敏感 query 键同样属于明文凭据。
fn contains_plaintext_credential(argument: &str) -> bool {
    url_has_plaintext_credential(argument)
        || argument
            .split_once('=')
            .is_some_and(|(_, value)| url_has_plaintext_credential(value))
}

fn url_has_plaintext_credential(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    !url.username().is_empty()
        || url.password().is_some()
        || url.query_pairs().any(|(name, _)| is_sensitive_name(&name))
}

fn is_sensitive_name(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('_', "-");
    matches!(
        normalized.as_str(),
        "api-key"
            | "apikey"
            | "access-key"
            | "accesskey"
            | "token"
            | "access-token"
            | "auth-token"
            | "refresh-token"
            | "secret"
            | "secret-key"
            | "client-secret"
            | "private-key"
            | "password"
            | "authorization"
            | "auth"
            | "bearer"
            | "credential"
            | "credentials"
            | "key"
            | "header"
            | "headers"
            | "signature"
            | "sig"
    )
}

fn validate_environment_references(
    environment: &BTreeMap<String, String>,
) -> Result<(), CatalogError> {
    if environment.iter().any(|(name, value)| {
        !is_environment_variable_name(name) || !is_environment_variable_name(value) || name != value
    }) {
        return Err(CatalogError::ContentInvalid);
    }
    Ok(())
}

fn is_environment_variable_name(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some('A'..='Z' | 'a'..='z' | '_'))
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
