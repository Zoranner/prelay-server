use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use prelay_protocol::ProviderProtocol;
use prelay_server::provider_catalog::ProviderCatalog;

fn write_catalog(language_models: &str, image_generation_models: &str, providers: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "prelay-provider-catalog-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
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
protocols = ["chat_completions", "responses", "anthropic_messages", "images_generations"]
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
            ProviderProtocol::AnthropicMessages,
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
