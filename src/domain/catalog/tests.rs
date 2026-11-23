use super::ProviderCatalog;

fn catalog() -> ProviderCatalog {
    crate::test_support::fixture_catalog()
}

#[test]
fn resolves_display_names_for_language_and_image_models() {
    let catalog = catalog();

    assert_eq!(catalog.model_display_name("gpt-5.6-luna"), "GPT-5.6 Luna");
    assert_eq!(catalog.model_display_name("gpt-image-1"), "GPT Image 1");
}

#[test]
fn falls_back_to_model_id_when_unknown() {
    let catalog = catalog();

    assert_eq!(catalog.model_display_name("unknown-model"), "unknown-model");
}

#[test]
fn response_contains_complete_model_catalog() {
    let catalog = catalog();
    let response = catalog.response();
    let language = response
        .language_models
        .iter()
        .find(|model| model.id == "gpt-5.6-luna")
        .expect("language model");
    let image = response
        .image_generation_models
        .iter()
        .find(|model| model.id == "gpt-image-1")
        .expect("image model");

    assert_eq!(language.display_name, "GPT-5.6 Luna");
    assert!(language.context_window.is_some());
    assert_eq!(image.display_name, "GPT Image 1");
    assert!(image
        .output_modalities
        .as_ref()
        .is_some_and(|modalities| modalities == &["image".to_string()]));
}

#[test]
fn hides_retired_providers_from_catalog_responses() {
    let catalog = catalog();
    let response = catalog.response();

    assert!(response
        .providers
        .iter()
        .all(|provider| provider.id != "retired_relay"));
    let deprecated = response
        .providers
        .iter()
        .find(|provider| provider.id == "legacy_relay")
        .expect("deprecated provider");
    assert!(deprecated.deprecated);
    assert!(catalog.provider_response("retired_relay").is_none());
    assert!(catalog.provider_response("legacy_relay").is_some());
}
