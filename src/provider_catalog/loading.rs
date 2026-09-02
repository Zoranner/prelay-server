use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

use super::{
    validation, CatalogImageGenerationModel, CatalogLanguageModel, CatalogProvider,
    ProviderCatalogError, ProviderProtocol, RawImageGenerationModel, RawLanguageModel, RawProvider,
};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageModelsDocument {
    #[serde(default)]
    models: Vec<RawLanguageModel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageGenerationModelsDocument {
    #[serde(default)]
    models: Vec<RawImageGenerationModel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvidersDocument {
    #[serde(default)]
    providers: Vec<RawProvider>,
}

pub(super) fn load_language_models(
    path: &Path,
) -> Result<BTreeMap<String, CatalogLanguageModel>, ProviderCatalogError> {
    let document: LanguageModelsDocument = parse_document(path)?;
    let mut models = BTreeMap::new();
    for model in document.models {
        let id = required_value("语言模型 ID", &model.id)?;
        let display_name = required_value("语言模型显示名称", &model.display_name)?;
        validation::validate_language_model(&id, &model)?;
        if models.contains_key(&id) {
            return Err(ProviderCatalogError(format!("语言模型 ID 重复: {id}")));
        }
        models.insert(
            id.clone(),
            CatalogLanguageModel {
                id,
                display_name,
                description: model.description,
                reasoning_efforts: model.reasoning_efforts,
                default_reasoning_effort: model.default_reasoning_effort,
                context_window: model.context_window,
                max_context_window: model.max_context_window,
                effective_context_window_percent: model.effective_context_window_percent,
                input_modalities: model.input_modalities,
                supports_parallel_tool_calls: model.supports_parallel_tool_calls,
                supports_reasoning_summaries: model.supports_reasoning_summaries,
                supports_image_detail_original: model.supports_image_detail_original,
                support_verbosity: model.support_verbosity,
                default_verbosity: model.default_verbosity,
                apply_patch_tool_type: model.apply_patch_tool_type,
                web_search_tool_type: model.web_search_tool_type,
                truncation_policy: model.truncation_policy,
                reasoning_summary_format: model.reasoning_summary_format,
                default_reasoning_summary: model.default_reasoning_summary,
                shell_type: model.shell_type,
                visibility: model.visibility,
                supported_in_api: model.supported_in_api,
                priority: model.priority,
                base_instructions: model.base_instructions,
                experimental_supported_tools: model.experimental_supported_tools,
                minimal_client_version: model.minimal_client_version,
            },
        );
    }
    Ok(models)
}

pub(super) fn load_image_generation_models(
    path: &Path,
) -> Result<BTreeMap<String, CatalogImageGenerationModel>, ProviderCatalogError> {
    let document: ImageGenerationModelsDocument = parse_document(path)?;
    let mut models = BTreeMap::new();
    for model in document.models {
        let id = required_value("图像生成模型 ID", &model.id)?;
        let display_name = required_value("图像生成模型显示名称", &model.display_name)?;
        validation::validate_image_generation_model(&id, &model)?;
        if models.contains_key(&id) {
            return Err(ProviderCatalogError(format!("图像生成模型 ID 重复: {id}")));
        }
        models.insert(
            id.clone(),
            CatalogImageGenerationModel {
                id,
                display_name,
                description: model.description,
                input_modalities: model.input_modalities,
                output_modalities: model.output_modalities,
                sizes: model.sizes,
                quality_options: model.quality_options,
                background_options: model.background_options,
                output_formats: model.output_formats,
                supports_editing: model.supports_editing,
                supports_mask: model.supports_mask,
                supports_reference_images: model.supports_reference_images,
                visibility: model.visibility,
                supported_in_api: model.supported_in_api,
                priority: model.priority,
            },
        );
    }
    Ok(models)
}

pub(super) fn load_providers(
    path: &Path,
    language_models: &BTreeMap<String, CatalogLanguageModel>,
    image_generation_models: &BTreeMap<String, CatalogImageGenerationModel>,
) -> Result<BTreeMap<String, CatalogProvider>, ProviderCatalogError> {
    let document: ProvidersDocument = parse_document(path)?;
    let mut providers = BTreeMap::new();
    for provider in document.providers {
        let id = required_value("供应商 ID", &provider.id)?;
        let name = required_value("供应商名称", &provider.name)?;
        let base_url = required_value("供应商默认 URL", &provider.base_url)?;
        validation::validate_protocols(&id, &provider.protocols)?;
        let protocol_base_urls =
            validation::validate_protocol_base_urls(&id, &provider, &base_url)?;
        validation::validate_provider_model_references(
            &id,
            "语言模型",
            &provider.language_models,
            language_models,
        )?;
        validation::validate_provider_model_references(
            &id,
            "图像生成模型",
            &provider.image_generation_models,
            image_generation_models,
        )?;
        if !provider.image_generation_models.is_empty()
            && !provider
                .protocols
                .contains(&ProviderProtocol::ImagesGenerations)
        {
            return Err(ProviderCatalogError(format!(
                "供应商 {id} 引用了图像生成模型但未声明 images_generations 协议"
            )));
        }
        if providers.contains_key(&id) {
            return Err(ProviderCatalogError(format!("供应商 ID 重复: {id}")));
        }
        providers.insert(
            id.clone(),
            CatalogProvider {
                id,
                name,
                auth_scheme: provider.auth_scheme,
                base_url,
                protocols: provider.protocols,
                protocol_base_urls,
                language_models: provider.language_models,
                image_generation_models: provider.image_generation_models,
            },
        );
    }
    Ok(providers)
}

fn parse_document<T: Default + for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<T, ProviderCatalogError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| ProviderCatalogError(format!("无法读取 {}: {error}", path.display())))?;
    if contents.trim().is_empty() {
        return Ok(T::default());
    }
    toml::from_str(&contents)
        .map_err(|error| ProviderCatalogError(format!("无法解析 {}: {error}", path.display())))
}

fn required_value(label: &str, value: &str) -> Result<String, ProviderCatalogError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ProviderCatalogError(format!("{label}不能为空")));
    }
    Ok(value.to_string())
}
