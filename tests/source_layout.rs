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

    let required_directories = ["anthropic", "responses", "stream/decode", "stream/encode"];

    for directory in required_directories {
        assert!(
            bridge_root.join(directory).is_dir(),
            "required bridge directory is missing: {directory}"
        );
    }

    assert_rust_files_within_limit(&bridge_root);
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
