use std::fs;
use std::path::Path;

#[test]
fn bridge_modules_use_directories_and_stay_within_limit() {
    let bridge_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bridge");
    let old_files = [
        "anthropic_decode.rs",
        "anthropic_encode.rs",
        "responses_decode.rs",
        "responses_encode.rs",
        "stream/decode_anthropic.rs",
        "stream/decode_chat.rs",
        "stream/decode_responses.rs",
        "stream/encode_anthropic.rs",
        "stream/encode_responses.rs",
    ];

    for old_file in old_files {
        assert!(
            !bridge_root.join(old_file).exists(),
            "obsolete bridge module still exists: {old_file}"
        );
    }

    assert_legacy_module_identifiers_absent(
        &bridge_root.join("mod.rs"),
        [
            "anthropic_decode",
            "anthropic_encode",
            "responses_decode",
            "responses_encode",
        ],
    );
    assert_legacy_module_identifiers_absent(
        &bridge_root.join("stream/mod.rs"),
        [
            "decode_anthropic",
            "decode_chat",
            "decode_responses",
            "encode_anthropic",
            "encode_responses",
        ],
    );

    let required_directories = ["anthropic", "responses", "stream/decode", "stream/encode"];

    for directory in required_directories {
        assert!(
            bridge_root.join(directory).is_dir(),
            "required bridge directory is missing: {directory}"
        );
    }

    assert_rust_files_within_limit(&bridge_root);
}

#[test]
fn provider_protocol_modules_use_directories_and_stay_within_limit() {
    let providers_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/providers");
    let old_files = ["chat_completions.rs", "spec.rs"];
    let required_files = [
        "chat_completions/request.rs",
        "chat_completions/response.rs",
        "chat_completions/stream.rs",
        "spec/capabilities.rs",
        "spec/urls.rs",
    ];

    for old_file in old_files {
        assert!(
            !providers_root.join(old_file).exists(),
            "obsolete provider protocol module still exists: {old_file}"
        );
    }

    for required_file in required_files {
        assert!(
            providers_root.join(required_file).is_file(),
            "required provider protocol module is missing: {required_file}"
        );
    }

    assert_rust_files_within_limit(&providers_root);
}

#[test]
fn chat_and_images_routes_use_protocol_directories_and_stay_within_limit() {
    let routes_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes/v1");
    let old_files = ["chat.rs", "images.rs"];
    let required_directories = ["chat", "images"];

    for old_file in old_files {
        assert!(
            !routes_root.join(old_file).exists(),
            "obsolete protocol route module still exists: {old_file}"
        );
    }

    for directory in required_directories {
        assert!(
            routes_root.join(directory).is_dir(),
            "required protocol route directory is missing: {directory}"
        );
    }

    assert_rust_files_within_limit(&routes_root.join("chat"));
    assert_rust_files_within_limit(&routes_root.join("images"));
}

#[test]
fn responses_and_messages_routes_use_protocol_directories_and_stay_within_limit() {
    let routes_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes/v1");
    let old_files = ["responses.rs", "messages.rs"];
    let required_files = [
        "responses/mod.rs",
        "responses/handler.rs",
        "responses/candidate.rs",
        "responses/native.rs",
        "responses/anthropic.rs",
        "responses/chat.rs",
        "responses/sessions.rs",
        "messages/mod.rs",
        "messages/handler.rs",
        "messages/candidate.rs",
        "messages/native.rs",
        "messages/responses.rs",
        "messages/chat.rs",
    ];

    for old_file in old_files {
        assert!(
            !routes_root.join(old_file).exists(),
            "obsolete protocol route module still exists: {old_file}"
        );
    }

    for required_file in required_files {
        assert!(
            routes_root.join(required_file).is_file(),
            "required protocol route module is missing: {required_file}"
        );
    }

    assert_rust_files_within_limit(&routes_root.join("responses"));
    assert_rust_files_within_limit(&routes_root.join("messages"));
}

#[test]
fn stream_observability_uses_directory_and_stays_within_limit() {
    let observability_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/observability");
    let stream_stats_root = observability_root.join("stream_stats");

    assert!(
        !observability_root.join("stream_stats.rs").exists(),
        "obsolete stream observability module still exists: stream_stats.rs"
    );
    assert!(
        stream_stats_root.is_dir(),
        "required stream observability directory is missing: stream_stats"
    );

    for required_file in [
        "mod.rs",
        "record.rs",
        "state.rs",
        "persistence.rs",
        "tests.rs",
    ] {
        assert!(
            stream_stats_root.join(required_file).is_file(),
            "required stream observability module is missing: {required_file}"
        );
    }

    assert_rust_files_within_limit(&stream_stats_root);
}

#[test]
fn persistence_modules_use_domain_directories_and_stay_within_limit() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let entity_root = source_root.join("entity");
    let schema_root = source_root.join("schema");
    let storage_root = source_root.join("storage");
    let obsolete_files = [
        "entity/identity_endpoint_configs.rs",
        "entity/identity_endpoint_model_routes.rs",
        "entity/identity_endpoint_models.rs",
        "entity/identity_model_aliases.rs",
        "entity/identity_provider_configs.rs",
        "entity/identity_provider_models.rs",
        "entity/identity_request_logs.rs",
        "entity/identity_response_sessions.rs",
        "schema.rs",
    ];
    let required_files = [
        "entity/identity/mod.rs",
        "entity/identity/endpoint_configs.rs",
        "entity/identity/endpoint_model_routes.rs",
        "entity/identity/endpoint_models.rs",
        "entity/identity/model_aliases.rs",
        "entity/identity/provider_configs.rs",
        "entity/identity/provider_models.rs",
        "entity/identity/request_logs.rs",
        "entity/identity/response_sessions.rs",
        "schema/mod.rs",
        "schema/indexes.rs",
        "schema/tables/mod.rs",
        "schema/tables/identity.rs",
        "schema/tables/providers.rs",
        "schema/tables/endpoints.rs",
        "schema/tables/sessions.rs",
        "schema/tables/request_logs.rs",
        "schema/tables/model_aliases.rs",
        "storage/access.rs",
        "storage/request_logs.rs",
    ];
    let mut violations = Vec::new();

    for obsolete_file in obsolete_files {
        if source_root.join(obsolete_file).exists() {
            violations.push(format!(
                "obsolete persistence module still exists: {obsolete_file}"
            ));
        }
    }
    for required_file in required_files {
        if !source_root.join(required_file).is_file() {
            violations.push(format!(
                "required persistence module is missing: {required_file}"
            ));
        }
    }
    for directory in [&entity_root, &schema_root, &storage_root] {
        if directory.is_dir() {
            collect_oversized_rust_files(directory, &mut violations);
        }
    }

    assert!(
        violations.is_empty(),
        "persistence source layout violations:\n{}",
        violations.join("\n")
    );
}

fn assert_legacy_module_identifiers_absent<I>(module_path: &Path, legacy_modules: I)
where
    I: IntoIterator<Item = &'static str>,
{
    let contents = fs::read_to_string(module_path).expect("bridge module must be readable");

    for legacy_module in legacy_modules {
        assert!(
            legacy_identifier_is_absent(&contents, legacy_module),
            "legacy bridge module identifier exists in {}: {legacy_module}",
            module_path.display()
        );
    }
}

fn legacy_identifier_is_absent(contents: &str, legacy_module: &str) -> bool {
    !contents.contains(legacy_module)
}

#[test]
fn rejects_legacy_module_identifiers_in_any_context() {
    for contents in [
        "pub(crate) mod legacy_name;",
        "// legacy_name must not return",
        "let example = \"legacy_name\";",
    ] {
        assert!(
            !legacy_identifier_is_absent(contents, "legacy_name"),
            "{contents}"
        );
    }
}

fn assert_rust_files_within_limit(directory: &Path) {
    for entry in fs::read_dir(directory).expect("bridge directory must be readable") {
        let entry = entry.expect("bridge directory entries must be readable");
        let path = entry.path();

        if path.is_dir() {
            assert_rust_files_within_limit(&path);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let contents = fs::read_to_string(&path).expect("bridge source must be readable");
            let line_count = contents.lines().count();
            assert!(
                line_count <= 450,
                "{} has {line_count} lines; maximum is 450",
                path.display()
            );
        }
    }
}

fn collect_oversized_rust_files(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("source directory must be readable") {
        let entry = entry.expect("source directory entries must be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_oversized_rust_files(&path, violations);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let contents = fs::read_to_string(&path).expect("source file must be readable");
            let line_count = contents.lines().count();
            if line_count > 450 {
                violations.push(format!(
                    "{} has {line_count} lines; maximum is 450",
                    path.display()
                ));
            }
        }
    }
}
