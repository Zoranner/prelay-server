use std::fs;
use std::path::Path;

const MAX_RUST_LINES: usize = 450;

#[test]
fn source_rust_files_stay_within_limit() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    collect_oversized_rust_files(&source_root, &mut violations);

    assert!(
        violations.is_empty(),
        "source line limit violations:\n{}",
        violations.join("\n")
    );
}

pub(super) fn collect_oversized_rust_files(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("source directory must be readable") {
        let entry = entry.expect("source directory entries must be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_oversized_rust_files(&path, violations);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let contents = fs::read_to_string(&path).expect("source file must be readable");
            let line_count = contents.lines().count();
            if line_count > MAX_RUST_LINES {
                violations.push(format!(
                    "{} has {line_count} lines; maximum is {MAX_RUST_LINES}",
                    path.display()
                ));
            }
        }
    }
}
