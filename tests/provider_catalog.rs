use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use prelay_protocol::ProviderProtocol;
use prelay_server::domain::catalog::{CatalogRequestHeaderValue, ProviderCatalog};

#[path = "provider_catalog/headers.rs"]
mod headers;
#[path = "provider_catalog/instructions.rs"]
mod instructions;
#[path = "provider_catalog/upstream_models.rs"]
mod upstream_models;

static CATALOG_DIRECTORY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn write_catalog(language_models: &str, image_generation_models: &str, providers: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "prelay-provider-catalog-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos(),
        CATALOG_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir_all(&directory).expect("create temporary catalog directory");
    let models_directory = directory.join("models");
    fs::create_dir_all(&models_directory).expect("create models directory");
    fs::write(models_directory.join("language.toml"), language_models)
        .expect("write language models catalog");
    fs::write(
        models_directory.join("image-generation.toml"),
        image_generation_models,
    )
    .expect("write image generation models catalog");
    fs::write(directory.join("providers.toml"), providers).expect("write provider catalog");
    directory
}

#[test]
fn loads_the_deployment_catalog() {
    ProviderCatalog::load(Path::new("config/catalog")).expect("load deployment catalog");
}

#[test]
fn deployment_catalog_excludes_retired_gpt_5_6_models() {
    let catalog =
        ProviderCatalog::load(Path::new("config/catalog")).expect("load deployment catalog");

    for model_id in ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"] {
        assert!(
            catalog.language_model(model_id).is_none(),
            "{model_id} should be retired"
        );
    }
}

#[test]
fn deployment_catalog_excludes_retired_supergpt_provider() {
    let catalog =
        ProviderCatalog::load(Path::new("config/catalog")).expect("load deployment catalog");

    assert!(catalog.provider("supergpt").is_none());
}

#[test]
fn deployment_catalog_excludes_retired_claude_models() {
    let catalog =
        ProviderCatalog::load(Path::new("config/catalog")).expect("load deployment catalog");

    for model_id in [
        "claude-fable-5-1",
        "claude-fable-5",
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-sonnet-5",
        "claude-haiku-5-5",
    ] {
        assert!(
            catalog.language_model(model_id).is_none(),
            "{model_id} should be retired"
        );
    }
}

#[test]
fn deployment_catalog_configures_opencode_go() {
    let catalog =
        ProviderCatalog::load(Path::new("config/catalog")).expect("load deployment catalog");
    let provider = catalog
        .provider("opencode_go")
        .expect("OpenCode Go provider");

    assert_eq!(provider.name, "OpenCode Go");
    assert_eq!(
        provider.base_url,
        "https://opencode.ai/inference/go/openai/v1"
    );
    assert_eq!(
        provider.protocols,
        vec![
            ProviderProtocol::ChatCompletions,
            ProviderProtocol::Responses,
        ]
    );
    assert_eq!(provider.request_headers.len(), 1);
    assert_eq!(
        provider.request_headers[0].name.as_str(),
        "x-opencode-session"
    );
    assert_eq!(
        provider.request_headers[0].value,
        CatalogRequestHeaderValue::SessionId
    );
    assert_eq!(catalog.provider_request_headers("opencode_go").len(), 1);
    assert!(catalog.provider_request_headers("deepseek").is_empty());
    assert_eq!(
        provider.language_models,
        vec![
            "gpt-6-luna",
            "deepseek-flash",
            "deepseek-v4-pro",
            "qwen3.8-max",
            "qwen3.8-flash",
            "k3",
            "glm-5.3",
            "glm-5.3-flash",
            "MiniMax-M3",
        ]
    );
    assert_eq!(
        catalog.provider_upstream_model("opencode_go", "deepseek-flash"),
        "deepseek-v4.1-flash"
    );
    assert_eq!(
        catalog.provider_upstream_model("opencode_go", "k3"),
        "kimi-k3"
    );
    assert_eq!(
        catalog.provider_upstream_model("opencode_go", "MiniMax-M3"),
        "minimax-m3"
    );
}

#[test]
fn loads_typed_models_and_ordered_provider_protocols() {
    let directory = write_catalog(
        "",
        r#"
[[models]]
id = "image-model"
display_name = "Image model"
input_modalities = ["text"]
output_modalities = ["image"]
"#,
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["chat_completions", "responses", "images_generations"]
image_generation_models = ["image-model"]
"#,
    );

    let catalog = ProviderCatalog::load(&directory).expect("load valid catalog");

    assert_eq!(
        catalog
            .image_generation_model("image-model")
            .expect("image model")
            .output_modalities,
        Some(vec!["image".to_string()])
    );
    assert_eq!(
        catalog.provider("provider").expect("provider").protocols,
        vec![
            ProviderProtocol::ChatCompletions,
            ProviderProtocol::Responses,
            ProviderProtocol::ImagesGenerations,
        ]
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn rejects_provider_protocols_out_of_standard_order() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
reasoning_efforts = ["none", "low", "medium", "high", "xhigh", "max"]
default_reasoning_effort = "medium"
"#,
        "",
        r#"
[[providers]]
id = "provider"
name = "Provider"
auth_scheme = "bearer"
base_url = "https://api.example.com/v1"
protocols = ["responses", "chat_completions"]
language_models = ["text-model"]
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject unordered protocols");

    assert!(error.to_string().contains("protocols"));
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}
