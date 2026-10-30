use std::fs;
use std::path::Path;

const LAYERS: [&str; 7] = [
    "bootstrap",
    "inbound",
    "service",
    "outbound",
    "store",
    "codec",
    "domain",
];

#[test]
fn layer_dependencies_point_downward() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    for layer in LAYERS {
        collect_layer_violations(
            &source_root,
            &source_root.join(layer),
            layer,
            allowed_layers(layer),
            &mut violations,
        );
    }

    assert!(
        violations.is_empty(),
        "layer dependency violations:\n{}",
        violations.join("\n")
    );
}

fn allowed_layers(layer: &str) -> &'static [&'static str] {
    match layer {
        "domain" => &["domain"],
        "codec" => &["codec", "domain"],
        "store" => &["store", "codec", "domain"],
        "outbound" => &["outbound", "codec", "domain"],
        "service" => &["service", "store", "outbound", "codec", "domain"],
        "inbound" => &["inbound", "service", "store", "outbound", "codec", "domain"],
        _ => &LAYERS,
    }
}

fn collect_layer_violations(
    source_root: &Path,
    directory: &Path,
    layer: &str,
    allowed: &[&str],
    violations: &mut Vec<String>,
) {
    for entry in fs::read_dir(directory).expect("source directory must be readable") {
        let entry = entry.expect("source directory entries must be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_layer_violations(source_root, &path, layer, allowed, violations);
            continue;
        }

        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }

        let mut contents = fs::read_to_string(&path).expect("source file must be readable");
        if let Some(test_start) = contents.find("#[cfg(test)]") {
            contents.truncate(test_start);
        }
        let normalized: String = contents.split_whitespace().collect();
        let relative_path = path
            .strip_prefix(source_root)
            .expect("source path must be below source root");

        for other in LAYERS {
            if other == layer || allowed.contains(&other) {
                continue;
            }
            let direct = format!("crate::{other}::");
            let grouped = format!("crate::{{{other}::");
            let grouped_item = format!(",{other}::");
            if normalized.contains(&direct)
                || normalized.contains(&grouped)
                || normalized.contains(&grouped_item)
            {
                violations.push(format!(
                    "{} in {layer} references higher layer {other}",
                    relative_path.display()
                ));
            }
        }
    }
}
