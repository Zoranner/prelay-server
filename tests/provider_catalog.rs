use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use prelay_protocol::ProviderProtocol;
use prelay_server::provider_catalog::ProviderCatalog;

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

#[test]
fn rejects_default_reasoning_effort_when_model_has_no_reasoning_efforts() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
reasoning_efforts = []
default_reasoning_effort = "max"
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
"#,
    );

    let error = ProviderCatalog::load(&directory).expect_err("reject inconsistent reasoning");

    assert!(error.to_string().contains("默认思考档位无效"));
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn loads_instructions_from_model_file_when_unset() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
base_instructions = ""
"#,
        "",
        "",
    );
    let instructions = directory.join("models/instructions");
    fs::create_dir_all(&instructions).expect("create instructions directory");
    fs::write(instructions.join("text-model.md"), "File instructions.\n")
        .expect("write instruction template");

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog
            .language_model("text-model")
            .and_then(|model| model.base_instructions.as_deref()),
        Some("File instructions.\n")
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn configured_base_instructions_win_over_model_file() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
base_instructions = "Configured instructions."
"#,
        "",
        "",
    );
    let instructions = directory.join("models/instructions");
    fs::create_dir_all(&instructions).expect("create instructions directory");
    fs::write(instructions.join("text-model.md"), "File instructions.\n")
        .expect("write instruction template");

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog
            .language_model("text-model")
            .and_then(|model| model.base_instructions.as_deref()),
        Some("Configured instructions.")
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn falls_back_to_default_instruction_file() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        "",
    );
    let instructions = directory.join("models/instructions");
    fs::create_dir_all(&instructions).expect("create instructions directory");
    fs::write(instructions.join("_default.md"), "Default instructions.\n")
        .expect("write default instruction template");

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog
            .language_model("text-model")
            .and_then(|model| model.base_instructions.as_deref()),
        Some("Default instructions.\n")
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn model_instruction_file_wins_over_default() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        "",
    );
    let instructions = directory.join("models/instructions");
    fs::create_dir_all(&instructions).expect("create instructions directory");
    fs::write(instructions.join("text-model.md"), "File instructions.\n")
        .expect("write instruction template");
    fs::write(instructions.join("_default.md"), "Default instructions.\n")
        .expect("write default instruction template");

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog
            .language_model("text-model")
            .and_then(|model| model.base_instructions.as_deref()),
        Some("File instructions.\n")
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

#[test]
fn keeps_base_instructions_unset_without_model_file() {
    let directory = write_catalog(
        r#"
[[models]]
id = "text-model"
display_name = "Text model"
"#,
        "",
        "",
    );

    let catalog = ProviderCatalog::load(&directory).expect("load catalog");

    assert_eq!(
        catalog
            .language_model("text-model")
            .and_then(|model| model.base_instructions.as_deref()),
        None
    );
    fs::remove_dir_all(directory).expect("remove temporary catalog directory");
}

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
