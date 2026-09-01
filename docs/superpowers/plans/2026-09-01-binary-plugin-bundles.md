# Binary-Safe Plugin Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make extension installation preserve every safe plugin file, including binary assets, without turning content decoding failures into catalog-unavailable errors.

**Architecture:** `ExtensionFile` transfers Base64-encoded source bytes as the cross-repository contract. The server reads and caches raw Gitea bytes, validates only locations and required text consumers, then serializes bytes. The client has one decode boundary and supplies the recovered bytes to rules, skills, and plugins; only rules require UTF-8 before writing.

**Tech Stack:** Rust, Serde, Base64 0.22, Axum, Reqwest, Tauri.

**Spec:** `docs/superpowers/specs/2026-08-31-binary-plugin-bundles-design.md`

## Global Constraints

- `ExtensionFile` has `path` and `content_base64`; the old `content` field is removed with no compatibility input.
- Plugin bundles include every existing path-safe blob from the fixed tag commit, regardless of file suffix.
- Gitea is accessed only by the server; the client only consumes the authenticated management API.
- Upstream Gitea failures remain `extension_catalog_unavailable` / HTTP 503.
- Invalid package content is `extension_content_invalid` / HTTP 422; client disk failures remain `local_extensions_error`.
- README stays a separate UTF-8 response; no credentials, deployment changes, or remote writes are in scope.

---

### Task 1: Publish the binary-safe protocol contract

**Files:**
- Modify: `prelay-protocol/src/extensions.rs`
- Modify: `prelay-protocol/src/error.rs`
- Modify: `prelay-protocol/tests/extensions.rs` or inline protocol tests if that is the established test location

**Interfaces:**
- Produces `ExtensionFile { path: String, content_base64: String }`.
- Produces `ProtocolErrorCode::ExtensionContentInvalid` with string value `extension_content_invalid`.

- [ ] **Step 1: Add failing JSON contract tests**

```rust
assert_eq!(
    serde_json::to_value(ExtensionFile {
        path: "assets/app-icon.png".to_string(),
        content_base64: "iVBORw0KGgo=".to_string(),
    })?,
    json!({ "path": "assets/app-icon.png", "contentBase64": "iVBORw0KGgo=" })
);
assert!(serde_json::from_value::<ExtensionFile>(json!({
    "path": "assets/app-icon.png", "content": "legacy"
})).is_err());
```

- [ ] **Step 2: Replace the DTO field and add the stable error code**

```rust
pub struct ExtensionFile {
    pub path: String,
    pub content_base64: String,
}
```

Add `ExtensionContentInvalid` to `ProtocolErrorCode` and return `"extension_content_invalid"` from `as_str`.

- [ ] **Step 3: Run protocol tests and commit**

```powershell
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git add src/extensions.rs src/error.rs tests/extensions.rs
git commit -m "支持二进制扩展文件传输"
```

### Task 2: Build and classify server installation bundles by bytes

**Files:**
- Modify: `prelay-server/crates/protocol` submodule pointer
- Modify: `prelay-server/src/extensions/gitea.rs`
- Modify: `prelay-server/src/extensions/catalog.rs`
- Modify: `prelay-server/src/routes/api/extensions.rs`
- Modify: `prelay-server/src/error.rs`
- Test: existing extension catalog and API route tests under `prelay-server/tests/` and `prelay-server/src/error.rs`

**Interfaces:**
- Consumes `ExtensionFile.content_base64` from Task 1.
- Changes `GiteaClient::read_file(...) -> Result<Vec<u8>>`.
- Adds `CatalogError::ContentInvalid` and maps it to the protocol error in Task 1.

- [ ] **Step 1: Write failing fixtures for raw PNG bytes and content-invalid errors**

Use a fixed `b"\x89PNG\r\n\x1a\n"` fixture in the catalog HTTP mock. Assert that a plugin installation bundle includes `assets/app-icon.png` with its Base64 bytes intact; assert malformed Base64, invalid required UTF-8 text, and illegal content paths map to `extension_content_invalid` with 422.

- [ ] **Step 2: Convert Gitea reads and file cache to raw bytes**

```rust
pub(super) async fn read_file(...) -> Result<Vec<u8>> {
    // Decode Gitea Base64 only; do not UTF-8 decode here.
}
```

Make `ExtensionCatalog::file` return cached `Vec<u8>`. In `readme`, decode those bytes with `String::from_utf8` and return `CatalogError::ContentInvalid` on failure.

- [ ] **Step 3: Serialize raw bytes and validate text-only consumers**

```rust
ExtensionFile {
    path,
    content_base64: BASE64.encode(self.file(...).await?),
}
```

Decode Base64 only for MCP manifest parsing and reject invalid text as `ContentInvalid`. Keep plugin enumeration as all existing safe blob paths and preserve the current Rule, Skill, and MCP path-selection boundaries.

- [ ] **Step 4: Map errors and verify server behavior**

Map `CatalogError::ContentInvalid` to `ExtensionContentInvalid`; map that protocol error to `StatusCode::UNPROCESSABLE_ENTITY` in `AppError`. Keep `CatalogError::Unavailable` at 503.

- [ ] **Step 5: Run focused and full server checks, then commit**

```powershell
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test extensions
git add crates/protocol src/extensions/gitea.rs src/extensions/catalog.rs src/routes/api/extensions.rs src/error.rs tests
git commit -m "保留插件安装包二进制文件"
```

### Task 3: Decode extension files once in the desktop client and write bytes unchanged

**Files:**
- Modify: `prelay-client/crates/protocol` submodule pointer
- Modify: `prelay-client/src-tauri/src/extensions/mod.rs`
- Modify: `prelay-client/src-tauri/src/extensions/rules.rs`
- Modify: `prelay-client/src-tauri/src/extensions/skills.rs`
- Modify: `prelay-client/src-tauri/src/extensions/plugins.rs`
- Test: the inline tests in those extension modules

**Interfaces:**
- Consumes `ExtensionFile.content_base64` from Task 1.
- Produces an internal decoded-file helper returning a byte slice or decoded byte vector with `ClientError::invalid_response` on malformed Base64.

- [ ] **Step 1: Add failing client tests**

```rust
let icon = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
install_codex_plugin(... bundle_with("assets/app-icon.png", BASE64.encode(icon)))?;
assert_eq!(fs::read(destination.join("assets/app-icon.png"))?, icon);
assert!(install_rule(... bundle_with("AGENTS.md", BASE64.encode([0xff]))).is_err());
```

- [ ] **Step 2: Introduce the single Base64 decode boundary**

Use the existing `base64` dependency in `extensions/mod.rs`. Decode `content_base64` with the standard engine, remove all direct `file.content.as_bytes()` access, and return `invalid_response` for malformed package data.

- [ ] **Step 3: Apply target-specific text requirements**

Rules decode then require `String::from_utf8` before writing. Skills and both plugin installers decode and call `atomic_write` with the original bytes. OpenCode continues selecting its executable plugin entries, while the Codex cache writes the complete safe bundle.

- [ ] **Step 4: Run client checks and commit**

```powershell
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test extensions
git add crates/protocol src-tauri/src/extensions
git commit -m "按原始字节安装扩展文件"
```

### Task 4: Verify the integrated contract and document deployment boundary

**Files:**
- Modify only if needed: `prelay-server/README.md` and `prelay-client/README.md`

**Interfaces:**
- Verifies the DTO from Task 1 is used by both updated submodules from Tasks 2 and 3.

- [ ] **Step 1: Verify both parent repositories reference the same protocol commit**

```powershell
git -C prelay-server submodule status crates/protocol
git -C prelay-client submodule status crates/protocol
```

- [ ] **Step 2: Run final verification per repository**

```powershell
git -C prelay-protocol diff --check
git -C prelay-server diff --check
git -C prelay-client diff --check
```

Run the focused protocol/server/client test suites and each repository's required `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings` checks.

- [ ] **Step 3: Report the deployment requirement**

State that the live service must run the committed server source (or a newly built and deployed image) and the desktop client must be rebuilt to consume the new DTO; do not claim the currently running 503 is fixed before that rollout and a real plugin installation succeeds.
