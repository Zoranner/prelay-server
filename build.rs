use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

fn main() {
    // 不使用 rerun-if-changed，改为自己比较文件时间戳。
    // 这样 Cargo 每次都会执行 build.rs，但 needs_rebuild() 是纯文件 stat，
    // 没有变化时几乎没有开销，且不会因为 prettier 修改源文件而死循环。

    if !needs_rebuild() {
        return;
    }

    if !Path::new("frontend/node_modules").exists() {
        progress("Installing dependencies...");
        run("bun", &["install"], "frontend");
    }

    progress("Formatting (prettier)...");
    run("bun", &["run", "format"], "frontend");

    progress("Linting (eslint --fix)...");
    run("bun", &["run", "lint:fix"], "frontend");

    progress("Building (vite)...");
    run("bun", &["run", "build"], "frontend");

    progress("Done.");
}

/// 判断是否需要重新构建前端。
/// 逻辑：静态产物不存在，或任何前端源文件比产物更新，则需要重建。
fn needs_rebuild() -> bool {
    let output = Path::new("static/index.html");
    let Ok(output_mtime) = output.metadata().and_then(|m| m.modified()) else {
        return true; // 产物缺失
    };

    file_newer("frontend/index.html", output_mtime)
        || file_newer("frontend/package.json", output_mtime)
        || file_newer("frontend/vite.config.ts", output_mtime)
        || file_newer("frontend/.prettierrc", output_mtime)
        || file_newer("frontend/eslint.config.js", output_mtime)
        || dir_newer("frontend/src", output_mtime)
}

fn file_newer(path: &str, than: SystemTime) -> bool {
    Path::new(path)
        .metadata()
        .and_then(|m| m.modified())
        .map(|t| t > than)
        .unwrap_or(false)
}

fn dir_newer(dir: &str, than: SystemTime) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let newer = if path.is_dir() {
            dir_newer(path.to_str().unwrap_or(""), than)
        } else {
            file_newer(path.to_str().unwrap_or(""), than)
        };
        if newer {
            return true;
        }
    }
    false
}

fn run(cmd: &str, args: &[&str], dir: &str) {
    let display = format!("{cmd} {}", args.join(" "));
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("Failed to start `{display}`: {e}"));
    if !status.success() {
        panic!("`{display}` failed with {status}");
    }
}

fn progress(msg: &str) {
    println!("cargo:warning=[frontend] {msg}");
}
