use std::fs;

use prelay_server::domain::catalog::ProviderCatalog;

use super::write_catalog;

#[test]
fn resolves_provider_upstream_model_names() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "relay"
name = "Relay"
auth_scheme = "bearer"
base_url = "https://relay.example/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.upstream_model_names]
text-model = "vendor-text-model"
"#,
    );

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog.provider_upstream_model("relay", "text-model"),
        "vendor-text-model"
    );
    assert_eq!(
        catalog.provider_upstream_model("relay", "unlisted-model"),
        "unlisted-model"
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn rejects_upstream_model_names_for_unreferenced_models() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        r#"
[[providers]]
id = "relay"
name = "Relay"
auth_scheme = "bearer"
base_url = "https://relay.example/v1"
protocols = ["chat_completions"]
language_models = ["text-model"]

[providers.upstream_model_names]
unlisted-model = "vendor-text-model"
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject unreferenced mapping");

    assert!(error.to_string().contains("上游名称"));
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}
