use std::fs;

use prelay_server::domain::catalog::ProviderCatalog;

use super::write_catalog;

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
fn deepseek_templates_stop_after_answering_questions() {
    for instructions in [
        include_str!("../../config/catalog/models/instructions/deepseek-v4-pro.md"),
        include_str!("../../config/catalog/models/instructions/deepseek-flash.md"),
    ] {
        assert!(
            !instructions.contains("先简短回答，再继续未完成的原任务"),
            "基础指令不能要求回答后继续原任务"
        );
        assert!(
            instructions.contains("提问、质疑和要方案不构成修改授权"),
            "基础指令必须明确提问不构成修改授权"
        );
        assert!(
            instructions.contains("先回答并停下"),
            "基础指令必须要求回答后停下"
        );
    }
}
