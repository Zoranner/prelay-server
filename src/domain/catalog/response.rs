use prelay_protocol::{
    CatalogImageGenerationModelResponse, CatalogLanguageModelResponse, CatalogProviderResponse,
    CatalogTruncationPolicyResponse, ProviderProtocolBaseUrl,
};

use super::{CatalogImageGenerationModel, CatalogLanguageModel, CatalogProvider};

pub(super) fn language_model_response(
    model: &CatalogLanguageModel,
) -> CatalogLanguageModelResponse {
    CatalogLanguageModelResponse {
        id: model.id.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        reasoning_efforts: model.reasoning_efforts.clone(),
        default_reasoning_effort: model.default_reasoning_effort.clone(),
        context_window: model.context_window,
        max_context_window: model.max_context_window,
        effective_context_window_percent: model.effective_context_window_percent,
        input_modalities: model.input_modalities.clone(),
        supports_parallel_tool_calls: model.supports_parallel_tool_calls,
        supports_reasoning_summaries: model.supports_reasoning_summaries,
        supports_image_detail_original: model.supports_image_detail_original,
        support_verbosity: model.support_verbosity,
        default_verbosity: model.default_verbosity.clone(),
        apply_patch_tool_type: model.apply_patch_tool_type.clone(),
        web_search_tool_type: model.web_search_tool_type.clone(),
        truncation_policy: model.truncation_policy.as_ref().map(|policy| {
            CatalogTruncationPolicyResponse {
                mode: policy.mode.clone(),
                limit: policy.limit,
            }
        }),
        reasoning_summary_format: model.reasoning_summary_format.clone(),
        default_reasoning_summary: model.default_reasoning_summary.clone(),
        shell_type: model.shell_type.clone(),
        visibility: model.visibility.clone(),
        supported_in_api: model.supported_in_api,
        priority: model.priority,
        base_instructions: model.base_instructions.clone(),
        experimental_supported_tools: model.experimental_supported_tools.clone(),
        minimal_client_version: model.minimal_client_version.clone(),
    }
}

pub(super) fn image_generation_model_response(
    model: &CatalogImageGenerationModel,
) -> CatalogImageGenerationModelResponse {
    CatalogImageGenerationModelResponse {
        id: model.id.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        input_modalities: model.input_modalities.clone(),
        output_modalities: model.output_modalities.clone(),
        sizes: model.sizes.clone(),
        quality_options: model.quality_options.clone(),
        background_options: model.background_options.clone(),
        output_formats: model.output_formats.clone(),
        supports_editing: model.supports_editing,
        supports_mask: model.supports_mask,
        supports_reference_images: model.supports_reference_images,
        visibility: model.visibility.clone(),
        supported_in_api: model.supported_in_api,
        priority: model.priority,
    }
}

pub(super) fn provider_response(provider: &CatalogProvider) -> CatalogProviderResponse {
    CatalogProviderResponse {
        id: provider.id.clone(),
        name: provider.name.clone(),
        auth_scheme: provider.auth_scheme,
        base_url: provider.base_url.clone(),
        protocols: provider.protocols.clone(),
        protocol_base_urls: provider
            .protocol_base_urls
            .iter()
            .map(|(protocol, base_url)| ProviderProtocolBaseUrl {
                protocol: *protocol,
                base_url: base_url.clone(),
            })
            .collect(),
        language_models: provider.language_models.clone(),
        image_generation_models: provider.image_generation_models.clone(),
    }
}
