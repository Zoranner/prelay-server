use std::path::Path;
use std::process::Command;

fn main() {
    // 告知 Cargo：以下文件/目录变化时重新执行 build script。
    // 目录路径会被递归监听。不声明则 Cargo 只在 Rust 源文件变化时才触发。
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/vite.config.ts");
    println!("cargo:rerun-if-changed=frontend/eslint.config.js");
    println!("cargo:rerun-if-changed=frontend/.prettierrc");

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
