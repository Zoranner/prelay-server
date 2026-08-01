use std::path::Path;
use std::process::Command;

fn main() {
    if std::env::var("SKIP_FRONTEND_BUILD").is_ok() {
        return;
    }

    // 告知 Cargo：以下文件/目录变化时重新执行 build script。
    // 目录路径会被递归监听。不声明则 Cargo 只在 Rust 源文件变化时才触发。
    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/bun.lock");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
    println!("cargo:rerun-if-changed=web/eslint.config.js");
    println!("cargo:rerun-if-changed=web/.prettierrc");

    if !Path::new("web/node_modules").exists() {
        panic!("web/node_modules is missing; run `cd web` then `bun install --frozen-lockfile`");
    }

    progress("Checking format (prettier)...");
    run("bun", &["run", "format:check"], "web");

    progress("Linting (eslint)...");
    run("bun", &["run", "lint"], "web");

    progress("Building (vite)...");
    run("bun", &["run", "build"], "web");

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
