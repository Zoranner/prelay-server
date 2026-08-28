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
