use std::collections::BTreeMap;

use prelay_protocol::{
    CatalogImageGenerationModelResponse, CatalogLanguageModelResponse, CatalogProviderResponse,
    ProviderAuthScheme, ProviderCatalogResponse, ProviderProtocol,
};
use serde::Deserialize;

mod loading;
mod response;
mod validation;

use loading::{load_image_generation_models, load_language_models, load_providers};
use response::{image_generation_model_response, language_model_response, provider_response};

const REASONING_EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
const INPUT_MODALITIES: [&str; 2] = ["text", "image"];
const OUTPUT_MODALITIES: [&str; 1] = ["image"];

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    language_models: BTreeMap<String, CatalogLanguageModel>,
    image_generation_models: BTreeMap<String, CatalogImageGenerationModel>,
    providers: BTreeMap<String, CatalogProvider>,
}

#[derive(Debug, Clone)]
pub struct CatalogLanguageModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub reasoning_efforts: Option<Vec<String>>,
    pub default_reasoning_effort: Option<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub effective_context_window_percent: Option<u8>,
    pub input_modalities: Option<Vec<String>>,
    pub supports_parallel_tool_calls: Option<bool>,
    pub supports_reasoning_summaries: Option<bool>,
    pub supports_image_detail_original: Option<bool>,
    pub support_verbosity: Option<bool>,
    pub default_verbosity: Option<String>,
    pub apply_patch_tool_type: Option<String>,
    pub web_search_tool_type: Option<String>,
    pub truncation_policy: Option<CatalogTruncationPolicy>,
    pub reasoning_summary_format: Option<String>,
    pub default_reasoning_summary: Option<String>,
    pub shell_type: Option<String>,
    pub visibility: Option<String>,
    pub supported_in_api: Option<bool>,
    pub priority: Option<u32>,
    pub base_instructions: Option<String>,
    pub experimental_supported_tools: Option<Vec<String>>,
    pub minimal_client_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogImageGenerationModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub sizes: Option<Vec<String>>,
    pub quality_options: Option<Vec<String>>,
    pub background_options: Option<Vec<String>>,
    pub output_formats: Option<Vec<String>>,
    pub supports_editing: Option<bool>,
    pub supports_mask: Option<bool>,
    pub supports_reference_images: Option<bool>,
    pub visibility: Option<String>,
    pub supported_in_api: Option<bool>,
    pub priority: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogTruncationPolicy {
    pub mode: String,
    pub limit: u64,
}

#[derive(Debug, Clone)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub auth_scheme: ProviderAuthScheme,
    pub base_url: String,
    pub protocols: Vec<ProviderProtocol>,
    pub protocol_base_urls: Vec<(ProviderProtocol, String)>,
    pub language_models: Vec<String>,
    pub image_generation_models: Vec<String>,
}

#[derive(Debug)]
pub struct ProviderCatalogError(pub(crate) String);

impl std::fmt::Display for ProviderCatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProviderCatalogError {}

#[derive(Debug, Deserialize)]
pub(super) struct RawLanguageModel {
    pub(super) id: String,
    pub(super) display_name: String,
    pub(super) description: Option<String>,
    pub(super) reasoning_efforts: Option<Vec<String>>,
    pub(super) default_reasoning_effort: Option<String>,
    pub(super) context_window: Option<u64>,
    pub(super) max_context_window: Option<u64>,
    pub(super) effective_context_window_percent: Option<u8>,
    pub(super) input_modalities: Option<Vec<String>>,
    pub(super) supports_parallel_tool_calls: Option<bool>,
    pub(super) supports_reasoning_summaries: Option<bool>,
    pub(super) supports_image_detail_original: Option<bool>,
    pub(super) support_verbosity: Option<bool>,
    pub(super) default_verbosity: Option<String>,
    pub(super) apply_patch_tool_type: Option<String>,
    pub(super) web_search_tool_type: Option<String>,
    pub(super) truncation_policy: Option<CatalogTruncationPolicy>,
    pub(super) reasoning_summary_format: Option<String>,
    pub(super) default_reasoning_summary: Option<String>,
    pub(super) shell_type: Option<String>,
    pub(super) visibility: Option<String>,
    pub(super) supported_in_api: Option<bool>,
    pub(super) priority: Option<u32>,
    pub(super) base_instructions: Option<String>,
    pub(super) experimental_supported_tools: Option<Vec<String>>,
    pub(super) minimal_client_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawImageGenerationModel {
    pub(super) id: String,
    pub(super) display_name: String,
    pub(super) description: Option<String>,
    pub(super) input_modalities: Option<Vec<String>>,
    pub(super) output_modalities: Option<Vec<String>>,
    pub(super) sizes: Option<Vec<String>>,
    pub(super) quality_options: Option<Vec<String>>,
    pub(super) background_options: Option<Vec<String>>,
    pub(super) output_formats: Option<Vec<String>>,
    pub(super) supports_editing: Option<bool>,
    pub(super) supports_mask: Option<bool>,
    pub(super) supports_reference_images: Option<bool>,
    pub(super) visibility: Option<String>,
    pub(super) supported_in_api: Option<bool>,
    pub(super) priority: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawProvider {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) auth_scheme: ProviderAuthScheme,
    pub(super) base_url: String,
    pub(super) protocols: Vec<ProviderProtocol>,
    #[serde(default)]
    pub(super) protocol_base_urls: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) language_models: Vec<String>,
    #[serde(default)]
    pub(super) image_generation_models: Vec<String>,
}

impl ProviderCatalog {
    pub fn load(directory: &std::path::Path) -> Result<Self, ProviderCatalogError> {
        let models_directory = directory.join("models");
        let language_models = load_language_models(&models_directory.join("language.toml"))?;
        let image_generation_models =
            load_image_generation_models(&models_directory.join("image-generation.toml"))?;
        let providers = load_providers(
            &directory.join("providers.toml"),
            &language_models,
            &image_generation_models,
        )?;
        Ok(Self {
            language_models,
            image_generation_models,
            providers,
        })
    }

    pub fn language_model(&self, model_id: &str) -> Option<&CatalogLanguageModel> {
        self.language_models.get(model_id)
    }

    pub fn image_generation_model(&self, model_id: &str) -> Option<&CatalogImageGenerationModel> {
        self.image_generation_models.get(model_id)
    }

    pub fn provider(&self, provider_id: &str) -> Option<&CatalogProvider> {
        self.providers.get(provider_id)
    }

    pub fn provider_supports_language_model(&self, provider_id: &str, model_id: &str) -> bool {
        self.provider(provider_id)
            .is_some_and(|provider| provider.language_models.iter().any(|id| id == model_id))
    }

    pub fn provider_supports_image_generation_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> bool {
        self.provider(provider_id).is_some_and(|provider| {
            provider
                .image_generation_models
                .iter()
                .any(|id| id == model_id)
        })
    }

    pub fn language_models(&self) -> Vec<CatalogLanguageModelResponse> {
        let mut models = self
            .language_models
            .values()
            .map(language_model_response)
            .collect::<Vec<_>>();
        models.sort_by_key(|model| (model.priority.unwrap_or(u32::MAX), model.id.clone()));
        models
    }

    pub fn image_generation_models(&self) -> Vec<CatalogImageGenerationModelResponse> {
        let mut models = self
            .image_generation_models
            .values()
            .map(image_generation_model_response)
            .collect::<Vec<_>>();
        models.sort_by_key(|model| (model.priority.unwrap_or(u32::MAX), model.id.clone()));
        models
    }

    pub fn providers(&self) -> Vec<CatalogProviderResponse> {
        self.providers.values().map(provider_response).collect()
    }

    pub fn language_model_response(&self, model_id: &str) -> Option<CatalogLanguageModelResponse> {
        self.language_models
            .get(model_id)
            .map(language_model_response)
    }

    pub fn image_generation_model_response(
        &self,
        model_id: &str,
    ) -> Option<CatalogImageGenerationModelResponse> {
        self.image_generation_models
            .get(model_id)
            .map(image_generation_model_response)
    }

    pub fn provider_response(&self, provider_id: &str) -> Option<CatalogProviderResponse> {
        self.providers.get(provider_id).map(provider_response)
    }

    pub fn response(&self) -> ProviderCatalogResponse {
        ProviderCatalogResponse {
            language_models: self.language_models(),
            image_generation_models: self.image_generation_models(),
            providers: self.providers(),
        }
    }
}
