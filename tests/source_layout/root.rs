use std::fs;
use std::path::Path;

#[test]
fn source_root_uses_layer_directories() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed_files = ["main.rs", "lib.rs", "error.rs"];
    let allowed_directories = [
        "bootstrap",
        "codec",
        "domain",
        "inbound",
        "outbound",
        "service",
        "store",
    ];
    let obsolete_paths = [
        "app.rs",
        "activity",
        "bridge",
        "client_update",
        "client_update.rs",
        "database.rs",
        "entity",
        "extensions",
        "failover.rs",
        "identity",
        "image_budget",
        "image_budget.rs",
        "memory",
        "models.rs",
        "observability",
        "protocol_body_limit.rs",
        "provider_catalog",
        "providers",
        "routes",
        "schema",
        "stats.rs",
        "storage",
        "upstream.rs",
    ];
    let mut violations = Vec::new();

    for entry in fs::read_dir(&source_root).expect("source directory must be readable") {
        let entry = entry.expect("source directory entries must be readable");
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("source entry name must be valid UTF-8");

        if path.is_dir() {
            if !allowed_directories.contains(&name) {
                violations.push(format!("unexpected top-level directory: {name}"));
            }
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !allowed_files.contains(&name)
        {
            violations.push(format!("unexpected root source file: {name}"));
        }
    }

    for obsolete_path in obsolete_paths {
        if source_root.join(obsolete_path).exists() {
            violations.push(format!(
                "obsolete source path still exists: {obsolete_path}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "source root layout violations:\n{}",
        violations.join("\n")
    );
}
