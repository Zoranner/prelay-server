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

    assert_legacy_module_declarations_absent(
        &bridge_root.join("mod.rs"),
        [
            "anthropic_decode",
            "anthropic_encode",
            "responses_decode",
            "responses_encode",
        ],
    );
    assert_legacy_module_declarations_absent(
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

fn assert_legacy_module_declarations_absent<I>(module_path: &Path, legacy_modules: I)
where
    I: IntoIterator<Item = &'static str>,
{
    let contents = fs::read_to_string(module_path).expect("bridge module must be readable");

    for legacy_module in legacy_modules {
        assert!(
            !contents
                .lines()
                .any(|line| line_declares_module(line, legacy_module)),
            "legacy bridge module declaration exists in {}: {legacy_module}",
            module_path.display()
        );
    }
}

fn line_declares_module(line: &str, module_name: &str) -> bool {
    let line = line.split_once("//").map_or(line, |(code, _)| code).trim();
    let Some(line) = strip_visibility(line) else {
        return false;
    };
    let Some(line) = strip_keyword(line, "mod") else {
        return false;
    };
    let Some(line) = line.trim_start().strip_prefix(module_name) else {
        return false;
    };

    !line
        .chars()
        .next()
        .is_some_and(|character| character.is_alphanumeric() || character == '_')
        && line.trim_start() == ";"
}

fn strip_visibility(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let Some(line) = strip_keyword(line, "pub") else {
        return Some(line);
    };
    let line = line.trim_start();

    if let Some(line) = line.strip_prefix('(') {
        let closing_parenthesis = line.find(')')?;
        return Some(line[closing_parenthesis + 1..].trim_start());
    }

    Some(line)
}

fn strip_keyword(line: &str, keyword: &str) -> Option<&str> {
    let line = line.strip_prefix(keyword)?;

    line.chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || character == '(')
        .then_some(line)
}

#[test]
fn recognizes_legacy_module_declarations_with_whitespace_and_visibility() {
    for line in [
        "mod legacy_name;",
        "  mod   legacy_name   ;",
        "pub mod legacy_name;",
        "pub(crate) mod legacy_name;",
        "pub ( crate ) mod legacy_name ; // compatibility",
    ] {
        assert!(line_declares_module(line, "legacy_name"), "{line}");
    }
}

#[test]
fn ignores_comments_strings_and_nonmatching_module_names() {
    for line in [
        "// pub mod legacy_name;",
        "let example = \"pub mod legacy_name;\";",
        "mod legacy_name_v2;",
        "mod legacy_name // incomplete declaration",
    ] {
        assert!(!line_declares_module(line, "legacy_name"), "{line}");
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
