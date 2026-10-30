use std::fs;

use prelay_server::domain::catalog::{CatalogRequestHeaderValue, ProviderCatalog};

use super::write_catalog;

#[test]
fn rejects_invalid_provider_request_headers() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.request_headers]
"bad header" = "value"
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject invalid request header");

    assert!(error.to_string().contains("请求头"), "{error}");
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn loads_literal_request_headers() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.request_headers]
x-api-version = "2026-10-28"
"#,
    );

    let catalog = ProviderCatalog::load(&directory).expect("load literal request header");
    let header = &catalog
        .provider("provider")
        .expect("provider")
        .request_headers[0];

    assert_eq!(header.name.as_str(), "x-api-version");
    assert_eq!(
        header.value,
        CatalogRequestHeaderValue::Literal("2026-10-28".to_string())
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn rejects_duplicate_request_headers_ignoring_case() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.request_headers]
"X-Test" = "first"
"x-test" = "second"
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject duplicate request headers");

    assert!(error.to_string().contains("重复声明"), "{error}");
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn rejects_unknown_request_header_sources() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.request_headers]
"x-session-id" = { source = "unknown" }
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject unknown header source");

    assert!(error.to_string().contains("来源"), "{error}");
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}
