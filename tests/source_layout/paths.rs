use std::fs;
use std::path::Path;

use super::limits::collect_oversized_rust_files;

const OBSOLETE_SOURCE_FILE_PREFIXES: [&str; 5] =
    ["identity_", "encode_", "decode_", "extensions_", "schema_"];

#[test]
fn source_modules_use_domain_directories() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let obsolete_files = [
        "bridge/anthropic_decode.rs",
        "bridge/anthropic_encode.rs",
        "bridge/anthropic/decode_tests.rs",
        "bridge/anthropic/mod.rs",
        "bridge/anthropic/decode.rs",
        "bridge/anthropic/encode.rs",
        "bridge/anthropic/tests.rs",
        "bridge/responses_decode.rs",
        "bridge/responses_encode.rs",
        "bridge/responses/decode_tests.rs",
        "bridge/stream/decode_anthropic.rs",
        "bridge/stream/decode/anthropic.rs",
        "bridge/stream/decode/anthropic_tests.rs",
        "bridge/stream/decode_chat.rs",
        "bridge/stream/decode_responses.rs",
        "bridge/stream/encode_anthropic.rs",
        "bridge/stream/encode/anthropic.rs",
        "bridge/stream/encode/anthropic_tests.rs",
        "bridge/stream/encode_responses.rs",
        "entity/identity_endpoint_configs.rs",
        "entity/identity_endpoint_model_routes.rs",
        "entity/identity_endpoint_models.rs",
        "entity/identity_model_aliases.rs",
        "entity/identity/model_aliases.rs",
        "entity/identity_provider_configs.rs",
        "entity/identity_provider_models.rs",
        "entity/identity_activities.rs",
        "entity/identity_response_sessions.rs",
        "observability/stream_stats.rs",
        "providers/chat_completions.rs",
        "providers/spec.rs",
        "routes/v1/chat.rs",
        "routes/v1/images.rs",
        "routes/v1/messages.rs",
        "routes/v1/messages/mod.rs",
        "routes/v1/messages/handler.rs",
        "routes/v1/messages/candidate.rs",
        "routes/v1/messages/chat.rs",
        "routes/v1/messages/native.rs",
        "routes/v1/messages/responses.rs",
        "routes/v1/messages/tests/activities.rs",
        "routes/v1/messages/tests/auth.rs",
        "routes/v1/messages/tests/candidates.rs",
        "routes/v1/messages/tests/fixtures.rs",
        "routes/v1/messages/tests/mod.rs",
        "routes/v1/messages/tests/routing.rs",
        "routes/v1/messages/tests/streaming.rs",
        "routes/v1/messages/tests/tools.rs",
        "routes/v1/responses.rs",
        "routes/v1/responses/anthropic.rs",
        "routes/v1/chat/tests/cases_a.rs",
        "routes/v1/chat/tests/cases_b.rs",
        "routes/v1/chat/tests/support.rs",
        "routes/v1/images/tests/cases_a.rs",
        "routes/v1/images/tests/cases_b.rs",
        "routes/v1/images/tests/support.rs",
        "routes/v1/messages/tests/cases_a.rs",
        "routes/v1/messages/tests/cases_b.rs",
        "routes/v1/messages/tests/support.rs",
        "routes/v1/responses/tests/cases_a.rs",
        "routes/v1/responses/tests/cases_b.rs",
        "routes/v1/responses/tests/support.rs",
        "schema.rs",
        "schema/tables/model_aliases.rs",
    ];
    let required_files = [
        "codec/bridge/responses/mod.rs",
        "codec/bridge/responses/decode.rs",
        "codec/bridge/responses/encode.rs",
        "codec/bridge/responses/tests.rs",
        "codec/bridge/stream/decode/mod.rs",
        "codec/bridge/stream/decode/chat.rs",
        "codec/bridge/stream/decode/responses.rs",
        "codec/bridge/stream/encode/mod.rs",
        "codec/bridge/stream/encode/responses.rs",
        "store/entity/identity/mod.rs",
        "store/entity/identity/endpoint_configs.rs",
        "store/entity/identity/endpoint_model_routes.rs",
        "store/entity/identity/endpoint_models.rs",
        "store/entity/identity/provider_configs.rs",
        "store/entity/identity/activities.rs",
        "store/entity/identity/response_sessions.rs",
        "observability/stream_stats/mod.rs",
        "observability/stream_stats/persistence.rs",
        "observability/stream_stats/record.rs",
        "observability/stream_stats/state.rs",
        "observability/stream_stats/tests.rs",
        "codec/providers/chat_completions/mod.rs",
        "codec/providers/chat_completions/request.rs",
        "codec/providers/chat_completions/response.rs",
        "codec/providers/chat_completions/stream.rs",
        "codec/providers/spec/mod.rs",
        "codec/providers/spec/capabilities.rs",
        "codec/providers/spec/urls.rs",
        "routes/v1/chat/mod.rs",
        "routes/v1/chat/handler.rs",
        "routes/v1/chat/candidate.rs",
        "routes/v1/chat/tests/auth.rs",
        "routes/v1/chat/tests/candidates.rs",
        "routes/v1/chat/tests/fixtures.rs",
        "routes/v1/chat/tests/activities.rs",
        "routes/v1/chat/tests/routing.rs",
        "routes/v1/chat/tests/streaming.rs",
        "routes/v1/images/mod.rs",
        "routes/v1/images/handler.rs",
        "routes/v1/images/candidate.rs",
        "routes/v1/images/activity.rs",
        "routes/v1/images/tests/candidates.rs",
        "routes/v1/images/tests/fixtures.rs",
        "routes/v1/images/tests/activities.rs",
        "routes/v1/images/tests/routing.rs",
        "routes/v1/images/tests/streaming.rs",
        "routes/v1/responses/mod.rs",
        "routes/v1/responses/handler.rs",
        "routes/v1/responses/candidate.rs",
        "routes/v1/responses/native.rs",
        "routes/v1/responses/chat.rs",
        "routes/v1/responses/sessions.rs",
        "routes/v1/responses/tests/auth.rs",
        "routes/v1/responses/tests/candidates.rs",
        "routes/v1/responses/tests/fixtures.rs",
        "routes/v1/responses/tests/activities.rs",
        "routes/v1/responses/tests/routing.rs",
        "routes/v1/responses/tests/sessions.rs",
        "routes/v1/responses/tests/streaming.rs",
        "store/schema/anthropic_removal.rs",
        "store/schema/mod.rs",
        "store/schema/indexes.rs",
        "store/schema/tables/mod.rs",
        "store/schema/tables/identity.rs",
        "store/schema/tables/providers.rs",
        "store/schema/tables/endpoints.rs",
        "store/schema/tables/sessions.rs",
        "store/schema/tables/activities.rs",
        "store/storage/access.rs",
        "store/storage/activities.rs",
    ];

    assert_paths_absent(&source_root, &obsolete_files, "obsolete source module");
    assert_paths_present(&source_root, &required_files, "required source module");
    assert_legacy_module_identifiers_absent(
        &source_root.join("codec/bridge/mod.rs"),
        [
            "anthropic_decode",
            "anthropic_encode",
            "responses_decode",
            "responses_encode",
        ],
    );
    assert_legacy_module_identifiers_absent(
        &source_root.join("codec/bridge/stream/mod.rs"),
        [
            "decode_anthropic",
            "decode_chat",
            "decode_responses",
            "encode_anthropic",
            "encode_responses",
        ],
    );
    for test_module in [
        "routes/v1/chat/tests/mod.rs",
        "routes/v1/images/tests/mod.rs",
        "routes/v1/responses/tests/mod.rs",
    ] {
        assert_legacy_module_identifiers_absent(&source_root.join(test_module), ["include!"]);
    }
}

#[test]
fn integration_tests_use_domain_directories_and_stay_within_limit() {
    let tests_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let obsolete_files = [
        "extensions_catalog.rs",
        "extensions_routes.rs",
        "identity_cleanup.rs",
        "identity_storage.rs",
        "identity/storage.rs",
        "management_isolation.rs",
        "protocol_routes.rs",
        "schema_contract.rs",
        "schema_initialization.rs",
        "v1_identity_scope.rs",
    ];
    let required_files = [
        "extensions.rs",
        "extensions/catalog.rs",
        "extensions/routes.rs",
        "identity.rs",
        "identity/cleanup.rs",
        "identity/storage/mod.rs",
        "identity/storage/candidates.rs",
        "identity/storage/credentials.rs",
        "identity/storage/fixtures.rs",
        "identity/storage/master_key.rs",
        "identity/storage/sessions.rs",
        "identity/storage/transactions.rs",
        "management.rs",
        "management/identity.rs",
        "management/providers.rs",
        "management/endpoints.rs",
        "management/stats.rs",
        "management/provider_operations.rs",
        "schema.rs",
        "schema/contract.rs",
        "schema/initialization.rs",
        "support/mod.rs",
        "support/auth.rs",
        "support/http.rs",
        "support/status.rs",
        "test_context/mod.rs",
        "v1.rs",
        "v1/identity_scope.rs",
        "v1/routes.rs",
    ];
    let mut violations = Vec::new();

    collect_obsolete_prefixed_integration_targets(&tests_root, &mut violations);
    collect_oversized_rust_files(&tests_root, &mut violations);
    for obsolete_file in obsolete_files {
        if tests_root.join(obsolete_file).exists() {
            violations.push(format!(
                "obsolete integration test still exists: {obsolete_file}"
            ));
        }
    }
    for required_file in required_files {
        if !tests_root.join(required_file).is_file() {
            violations.push(format!(
                "required integration test is missing: {required_file}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "integration test layout violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn source_rust_files_do_not_encode_directory_layers_in_names() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    collect_obsolete_prefixed_source_files(&source_root, &source_root, &mut violations);

    assert!(
        violations.is_empty(),
        "obsolete prefixed source files:\n{}",
        violations.join("\n")
    );
}

#[test]
fn recognizes_all_obsolete_source_file_prefixes() {
    for file_name in [
        "identity_provider.rs",
        "encode_responses.rs",
        "decode_responses.rs",
        "extensions_catalog.rs",
        "schema_tables.rs",
    ] {
        assert!(has_obsolete_source_file_prefix(Path::new(file_name)));
    }
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

fn assert_paths_absent(root: &Path, relative_paths: &[&str], kind: &str) {
    for relative_path in relative_paths {
        assert!(
            !root.join(relative_path).exists(),
            "{kind} still exists: {relative_path}"
        );
    }
}

fn assert_paths_present(root: &Path, relative_paths: &[&str], kind: &str) {
    for relative_path in relative_paths {
        assert!(
            root.join(relative_path).is_file(),
            "{kind} is missing: {relative_path}"
        );
    }
}

fn assert_legacy_module_identifiers_absent<I>(module_path: &Path, legacy_modules: I)
where
    I: IntoIterator<Item = &'static str>,
{
    let contents = fs::read_to_string(module_path).expect("module must be readable");

    for legacy_module in legacy_modules {
        assert!(
            legacy_identifier_is_absent(&contents, legacy_module),
            "legacy module identifier exists in {}: {legacy_module}",
            module_path.display()
        );
    }
}

fn legacy_identifier_is_absent(contents: &str, legacy_module: &str) -> bool {
    !contents.contains(legacy_module)
}

fn collect_obsolete_prefixed_integration_targets(tests_root: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(tests_root).expect("test directory must be readable") {
        let entry = entry.expect("test directory entries must be readable");
        let path = entry.path();

        if path.extension().is_some_and(|extension| extension == "rs") {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("test file name must be valid UTF-8");
            if ["extensions_", "identity_", "management_", "schema_"]
                .iter()
                .any(|prefix| file_name.starts_with(prefix))
            {
                let relative_path = path
                    .strip_prefix(tests_root)
                    .expect("test path must be below test root");
                violations.push(format!(
                    "obsolete prefixed integration test exists: {}",
                    relative_path.display()
                ));
            }
        }
    }
}

fn collect_obsolete_prefixed_source_files(
    source_root: &Path,
    directory: &Path,
    violations: &mut Vec<String>,
) {
    for entry in fs::read_dir(directory).expect("source directory must be readable") {
        let entry = entry.expect("source directory entries must be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_obsolete_prefixed_source_files(source_root, &path, violations);
        } else if has_obsolete_source_file_prefix(&path) {
            let relative_path = path
                .strip_prefix(source_root)
                .expect("source path must be below source root");
            violations.push(relative_path.display().to_string());
        }
    }
}

fn has_obsolete_source_file_prefix(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|file_name| {
                OBSOLETE_SOURCE_FILE_PREFIXES
                    .iter()
                    .any(|prefix| file_name.starts_with(prefix))
            })
}
