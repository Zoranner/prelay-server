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
    command
        .iter()
        .skip(1)
        .any(|argument| is_sensitive_command_option(argument))
}

fn is_sensitive_command_option(argument: &str) -> bool {
    let option = argument
        .trim_start_matches('-')
        .split_once('=')
        .map_or(argument.trim_start_matches('-'), |(name, _)| name);
    is_sensitive_name(option)
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
            | "secret"
            | "password"
            | "authorization"
            | "auth"
            | "credential"
            | "credentials"
            | "key"
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
mod tests {
    use std::collections::BTreeMap;

    use prelay_protocol::{ExtensionMcpManifest, ExtensionMcpTransport};

    use super::validate_mcp_manifest;

    #[test]
    fn accepts_only_safe_and_usable_mcp_connection_details() {
        let valid_stdio = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string(), "mcp-server-filesystem".to_string()],
                cwd: None,
                environment: BTreeMap::from([(
                    "GITHUB_TOKEN".to_string(),
                    "GITHUB_TOKEN".to_string(),
                )]),
                enabled: true,
                timeout_ms: Some(30_000),
            },
        };
        let empty_command = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: Vec::new(),
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_environment = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string()],
                cwd: None,
                environment: BTreeMap::from([(
                    "GITHUB_TOKEN".to_string(),
                    "plain-text-secret".to_string(),
                )]),
                enabled: true,
                timeout_ms: None,
            },
        };
        let remapped_environment = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string()],
                cwd: None,
                environment: BTreeMap::from([(
                    "GITHUB_TOKEN".to_string(),
                    "PRELAY_GITHUB_TOKEN".to_string(),
                )]),
                enabled: true,
                timeout_ms: None,
            },
        };
        let working_directory = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string()],
                cwd: Some("C:/workspace".to_string()),
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let invalid_url = ExtensionMcpManifest {
            name: "remote".to_string(),
            transport: ExtensionMcpTransport::Http {
                url: "file:///C:/server".to_string(),
                headers: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_header = ExtensionMcpManifest {
            name: "remote".to_string(),
            transport: ExtensionMcpTransport::Http {
                url: "https://mcp.example.test".to_string(),
                headers: BTreeMap::from([(
                    "Authorization".to_string(),
                    "Bearer plain-text-secret".to_string(),
                )]),
                enabled: true,
                timeout_ms: None,
            },
        };
        let disabled_stdio = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string(), "mcp-server-filesystem".to_string()],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: false,
                timeout_ms: None,
            },
        };
        let invalid_server_name = ExtensionMcpManifest {
            name: "filesystem server".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string(), "mcp-server-filesystem".to_string()],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let reserved_server_name = ExtensionMcpManifest {
            name: "workspace".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec!["uvx".to_string(), "mcp-server-filesystem".to_string()],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_command_secret = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec![
                    "uvx".to_string(),
                    "mcp-server-filesystem".to_string(),
                    "--api-key".to_string(),
                    "plain-text-secret".to_string(),
                ],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_url_secret = ExtensionMcpManifest {
            name: "remote".to_string(),
            transport: ExtensionMcpTransport::Http {
                url: "https://mcp.example.test?token=plain-text-secret".to_string(),
                headers: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_url_api_key = ExtensionMcpManifest {
            name: "remote".to_string(),
            transport: ExtensionMcpTransport::Http {
                url: "https://mcp.example.test?api_key=plain-text-secret".to_string(),
                headers: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let plaintext_access_key = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec![
                    "uvx".to_string(),
                    "mcp-server-filesystem".to_string(),
                    "--access-key".to_string(),
                    "plain-text-secret".to_string(),
                ],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let url_with_query = ExtensionMcpManifest {
            name: "remote".to_string(),
            transport: ExtensionMcpTransport::Http {
                url: "https://mcp.example.test?transport=stream".to_string(),
                headers: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };
        let ordinary_command_argument = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: vec![
                    "uvx".to_string(),
                    "mcp-server-filesystem".to_string(),
                    "--tokenizer=cl100k_base".to_string(),
                ],
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };

        assert!(validate_mcp_manifest(&valid_stdio).is_ok());
        assert!(validate_mcp_manifest(&empty_command).is_err());
        assert!(validate_mcp_manifest(&plaintext_environment).is_err());
        assert!(validate_mcp_manifest(&remapped_environment).is_err());
        assert!(validate_mcp_manifest(&working_directory).is_err());
        assert!(validate_mcp_manifest(&invalid_url).is_err());
        assert!(validate_mcp_manifest(&plaintext_header).is_err());
        assert!(validate_mcp_manifest(&disabled_stdio).is_err());
        assert!(validate_mcp_manifest(&invalid_server_name).is_err());
        assert!(validate_mcp_manifest(&reserved_server_name).is_err());
        assert!(validate_mcp_manifest(&plaintext_command_secret).is_err());
        assert!(validate_mcp_manifest(&plaintext_url_secret).is_err());
        assert!(validate_mcp_manifest(&plaintext_url_api_key).is_err());
        assert!(validate_mcp_manifest(&plaintext_access_key).is_err());
        assert!(validate_mcp_manifest(&url_with_query).is_err());
        assert!(validate_mcp_manifest(&ordinary_command_argument).is_ok());
    }
}
