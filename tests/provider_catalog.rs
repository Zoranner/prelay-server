use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use prelay_protocol::{ModelType, ProviderProtocol};
use prelay_server::provider_catalog::ProviderCatalog;

fn write_catalog(models: &str, providers: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "prelay-provider-catalog-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir_all(&directory).expect("create temporary catalog directory");
    fs::write(directory.join("models.toml"), models).expect("write models catalog");
    fs::write(directory.join("providers.toml"), providers).expect("write providers catalog");
    directory
}

#[test]
fn loads_the_default_deployment_catalog() {
    let catalog = ProviderCatalog::load(Path::new("deploy/app/config"))
        .expect("load default deployment catalog");

    assert!(catalog.provider("gotoken").is_some());
    assert!(catalog.provider("deepseek").is_some());
    assert_eq!(
        catalog
            .model("gpt-image-1")
            .expect("image model")
            .model_type,
        ModelType::Image
    );
}

#[test]
fn loads_typed_models_and_ordered_provider_protocols() {
    let directory = write_catalog(
        r#"
[[models]]
id = "gpt-image-1"
display_name = "GPT Image 1"
model_type = "image"
reasoning_efforts = []
"#,
        r#"
[[providers]]
id = "gotoken"
name = "GoToken 套餐"
auth_scheme = "bearer"
base_url = "https://gotoken.cc"
protocols = ["chat_completions", "responses", "anthropic_messages", "images_generations"]
models = ["gpt-image-1"]
"#,
    );

    let catalog = ProviderCatalog::load(&directory).expect("load valid catalog");

    assert_eq!(
        catalog
            .model("gpt-image-1")
            .expect("image model")
            .model_type,
        ModelType::Image
    );
    assert_eq!(
        catalog
            .provider("gotoken")
            .expect("GoToken provider")
            .protocols,
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
id = "gpt-5.6-luna"
display_name = "GPT-5.6 Luna"
model_type = "text"
reasoning_efforts = ["low", "medium", "high"]
default_reasoning_effort = "medium"
"#,
        r#"
[[providers]]
id = "gotoken"
name = "GoToken 套餐"
auth_scheme = "bearer"
base_url = "https://gotoken.cc"
protocols = ["responses", "chat_completions"]
models = ["gpt-5.6-luna"]
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject unordered protocols");

    assert!(error.to_string().contains("protocols"));
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}
