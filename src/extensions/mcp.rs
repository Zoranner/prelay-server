use std::collections::BTreeMap;

use prelay_protocol::{ExtensionMcpManifest, ExtensionMcpTransport};

use super::CatalogError;

pub(super) fn validate_mcp_manifest(manifest: &ExtensionMcpManifest) -> Result<(), CatalogError> {
    if manifest.name.trim().is_empty() || manifest.name.chars().any(char::is_control) {
        return Err(CatalogError::ContentInvalid);
    }

    match &manifest.transport {
        ExtensionMcpTransport::Stdio {
            command,
            cwd,
            environment,
            ..
        } => {
            if command
                .first()
                .is_none_or(|program| program.trim().is_empty())
                || cwd.is_some()
            {
                return Err(CatalogError::ContentInvalid);
            }
            validate_environment_references(environment)
        }
        ExtensionMcpTransport::Http { url, headers, .. } => {
            let url = reqwest::Url::parse(url).map_err(|_| CatalogError::ContentInvalid)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
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

        assert!(validate_mcp_manifest(&valid_stdio).is_ok());
        assert!(validate_mcp_manifest(&empty_command).is_err());
        assert!(validate_mcp_manifest(&plaintext_environment).is_err());
        assert!(validate_mcp_manifest(&remapped_environment).is_err());
        assert!(validate_mcp_manifest(&working_directory).is_err());
        assert!(validate_mcp_manifest(&invalid_url).is_err());
        assert!(validate_mcp_manifest(&plaintext_header).is_err());
    }
}
