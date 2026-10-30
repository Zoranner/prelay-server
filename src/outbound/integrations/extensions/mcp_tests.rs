use std::collections::BTreeMap;

use prelay_protocol::{ExtensionMcpManifest, ExtensionMcpTransport};

use super::validate_mcp_manifest;

#[test]
fn rejects_plaintext_credentials_in_command_arguments() {
    for command in [
        vec![
            "uvx",
            "mcp-remote",
            "--header",
            "Authorization: Bearer plain-text-secret",
        ],
        vec![
            "uvx",
            "mcp-remote",
            "https://user:plain-text-secret@mcp.example.test/mcp",
        ],
        vec![
            "uvx",
            "mcp-remote",
            "https://mcp.example.test/mcp?access_token=plain-text-secret",
        ],
        vec![
            "uvx",
            "mcp-server-filesystem",
            "--client-secret=plain-text-secret",
        ],
    ] {
        let manifest = ExtensionMcpManifest {
            name: "filesystem".to_string(),
            transport: ExtensionMcpTransport::Stdio {
                command: command
                    .iter()
                    .map(|argument| argument.to_string())
                    .collect(),
                cwd: None,
                environment: BTreeMap::new(),
                enabled: true,
                timeout_ms: None,
            },
        };

        assert!(validate_mcp_manifest(&manifest).is_err());
    }
}

#[test]
fn accepts_a_credential_free_remote_url_argument() {
    let manifest = ExtensionMcpManifest {
        name: "filesystem".to_string(),
        transport: ExtensionMcpTransport::Stdio {
            command: vec![
                "uvx".to_string(),
                "mcp-remote".to_string(),
                "https://docs.example.test/guide".to_string(),
            ],
            cwd: None,
            environment: BTreeMap::new(),
            enabled: true,
            timeout_ms: None,
        },
    };

    assert!(validate_mcp_manifest(&manifest).is_ok());
}

#[test]
fn accepts_only_safe_and_usable_mcp_connection_details() {
    let valid_stdio = ExtensionMcpManifest {
        name: "filesystem".to_string(),
        transport: ExtensionMcpTransport::Stdio {
            command: vec!["uvx".to_string(), "mcp-server-filesystem".to_string()],
            cwd: None,
            environment: BTreeMap::from([("GITHUB_TOKEN".to_string(), "GITHUB_TOKEN".to_string())]),
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
