use std::{
    collections::{BTreeMap, HashSet},
    fmt, fs,
    path::Path,
};

use prelay_protocol::{
    CatalogImageGenerationModelResponse, CatalogLanguageModelResponse, CatalogProviderResponse,
    CatalogTruncationPolicyResponse, ProviderAuthScheme, ProviderCatalogResponse, ProviderProtocol,
    ProviderProtocolBaseUrl,
};
use serde::Deserialize;

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
pub struct ProviderCatalogError(String);

impl fmt::Display for ProviderCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProviderCatalogError {}

impl ProviderCatalog {
    pub fn load(directory: &Path) -> Result<Self, ProviderCatalogError> {
        let models_directory = directory.join("models");
        let language_models = load_language_models(&models_directory.join("language.toml"))?;
        let image_generation_models =
            load_image_generation_models(&models_directory.join("image-generation.toml"))?;
        let providers = load_providers(
            &directory.join("provider-catalog.toml"),
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

    pub fn response(&self) -> ProviderCatalogResponse {
        let mut language_models = self
            .language_models
            .values()
            .map(language_model_response)
            .collect::<Vec<_>>();
        language_models.sort_by_key(|model| (model.priority.unwrap_or(u32::MAX), model.id.clone()));

        let mut image_generation_models = self
            .image_generation_models
            .values()
            .map(image_generation_model_response)
            .collect::<Vec<_>>();
        image_generation_models
            .sort_by_key(|model| (model.priority.unwrap_or(u32::MAX), model.id.clone()));

        ProviderCatalogResponse {
            language_models,
            image_generation_models,
            providers: self.providers.values().map(provider_response).collect(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageModelsDocument {
    #[serde(default)]
    models: Vec<RawLanguageModel>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLanguageModel {
    id: String,
    display_name: String,
    description: Option<String>,
    reasoning_efforts: Option<Vec<String>>,
    default_reasoning_effort: Option<String>,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    effective_context_window_percent: Option<u8>,
    input_modalities: Option<Vec<String>>,
    supports_parallel_tool_calls: Option<bool>,
    supports_reasoning_summaries: Option<bool>,
    supports_image_detail_original: Option<bool>,
    support_verbosity: Option<bool>,
    default_verbosity: Option<String>,
    apply_patch_tool_type: Option<String>,
    web_search_tool_type: Option<String>,
    truncation_policy: Option<CatalogTruncationPolicy>,
    reasoning_summary_format: Option<String>,
    default_reasoning_summary: Option<String>,
    shell_type: Option<String>,
    visibility: Option<String>,
    supported_in_api: Option<bool>,
    priority: Option<u32>,
    base_instructions: Option<String>,
    experimental_supported_tools: Option<Vec<String>>,
    minimal_client_version: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageGenerationModelsDocument {
    #[serde(default)]
    models: Vec<RawImageGenerationModel>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawImageGenerationModel {
    id: String,
    display_name: String,
    description: Option<String>,
    input_modalities: Option<Vec<String>>,
    output_modalities: Option<Vec<String>>,
    sizes: Option<Vec<String>>,
    quality_options: Option<Vec<String>>,
    background_options: Option<Vec<String>>,
    output_formats: Option<Vec<String>>,
    supports_editing: Option<bool>,
    supports_mask: Option<bool>,
    supports_reference_images: Option<bool>,
    visibility: Option<String>,
    supported_in_api: Option<bool>,
    priority: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvidersDocument {
    #[serde(default)]
    providers: Vec<RawProvider>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    id: String,
    name: String,
    auth_scheme: ProviderAuthScheme,
    base_url: String,
    protocols: Vec<ProviderProtocol>,
    #[serde(default)]
    protocol_base_urls: BTreeMap<String, String>,
    #[serde(default)]
    language_models: Vec<String>,
    #[serde(default)]
    image_generation_models: Vec<String>,
}

fn load_language_models(
    path: &Path,
) -> Result<BTreeMap<String, CatalogLanguageModel>, ProviderCatalogError> {
    let document: LanguageModelsDocument = parse_document(path)?;
    let mut models = BTreeMap::new();
    for model in document.models {
        let id = required_value("语言模型 ID", &model.id)?;
        let display_name = required_value("语言模型显示名称", &model.display_name)?;
        validate_language_model(&id, &model)?;
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

fn load_image_generation_models(
    path: &Path,
) -> Result<BTreeMap<String, CatalogImageGenerationModel>, ProviderCatalogError> {
    let document: ImageGenerationModelsDocument = parse_document(path)?;
    let mut models = BTreeMap::new();
    for model in document.models {
        let id = required_value("图像生成模型 ID", &model.id)?;
        let display_name = required_value("图像生成模型显示名称", &model.display_name)?;
        validate_image_generation_model(&id, &model)?;
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

fn load_providers(
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
        validate_protocols(&id, &provider.protocols)?;
        let protocol_base_urls = validate_protocol_base_urls(&id, &provider, &base_url)?;
        validate_provider_model_references(
            &id,
            "语言模型",
            &provider.language_models,
            language_models,
        )?;
        validate_provider_model_references(
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

fn validate_language_model(
    model_id: &str,
    model: &RawLanguageModel,
) -> Result<(), ProviderCatalogError> {
    validate_context_window(model_id, model.context_window, model.max_context_window)?;
    if model
        .effective_context_window_percent
        .is_some_and(|percent| percent == 0 || percent > 100)
    {
        return Err(ProviderCatalogError(format!(
            "语言模型 {model_id} 的有效上下文比例无效"
        )));
    }
    if model
        .truncation_policy
        .as_ref()
        .is_some_and(|policy| policy.mode.trim().is_empty() || policy.limit == 0)
    {
        return Err(ProviderCatalogError(format!(
            "语言模型 {model_id} 的截断策略无效"
        )));
    }
    validate_modalities(
        model_id,
        "输入",
        model.input_modalities.as_deref(),
        &INPUT_MODALITIES,
    )?;
    validate_reasoning(
        model_id,
        model.reasoning_efforts.as_deref(),
        model.default_reasoning_effort.as_deref(),
    )
}

fn validate_image_generation_model(
    model_id: &str,
    model: &RawImageGenerationModel,
) -> Result<(), ProviderCatalogError> {
    validate_modalities(
        model_id,
        "输入",
        model.input_modalities.as_deref(),
        &INPUT_MODALITIES,
    )?;
    validate_modalities(
        model_id,
        "输出",
        model.output_modalities.as_deref(),
        &OUTPUT_MODALITIES,
    )?;
    for (label, values) in [
        ("尺寸", model.sizes.as_deref()),
        ("质量", model.quality_options.as_deref()),
        ("背景", model.background_options.as_deref()),
        ("输出格式", model.output_formats.as_deref()),
    ] {
        validate_nonempty_values(model_id, label, values)?;
    }
    Ok(())
}

fn validate_context_window(
    model_id: &str,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
) -> Result<(), ProviderCatalogError> {
    if context_window == Some(0) {
        return Err(ProviderCatalogError(format!(
            "语言模型 {model_id} 的上下文窗口必须大于零"
        )));
    }
    if max_context_window == Some(0)
        || max_context_window
            .is_some_and(|maximum| context_window.is_some_and(|current| maximum < current))
    {
        return Err(ProviderCatalogError(format!(
            "语言模型 {model_id} 的最大上下文窗口无效"
        )));
    }
    Ok(())
}

fn validate_modalities(
    model_id: &str,
    label: &str,
    modalities: Option<&[String]>,
    allowed: &[&str],
) -> Result<(), ProviderCatalogError> {
    let Some(modalities) = modalities else {
        return Ok(());
    };
    let unique = modalities
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if unique.len() != modalities.len()
        || modalities
            .iter()
            .any(|value| !allowed.contains(&value.as_str()))
    {
        return Err(ProviderCatalogError(format!(
            "模型 {model_id} 的{label}模态无效"
        )));
    }
    Ok(())
}

fn validate_nonempty_values(
    model_id: &str,
    label: &str,
    values: Option<&[String]>,
) -> Result<(), ProviderCatalogError> {
    let Some(values) = values else {
        return Ok(());
    };
    let unique = values.iter().map(String::as_str).collect::<HashSet<_>>();
    if values.iter().any(|value| value.trim().is_empty()) || unique.len() != values.len() {
        return Err(ProviderCatalogError(format!(
            "图像生成模型 {model_id} 的{label}配置无效"
        )));
    }
    Ok(())
}

fn validate_reasoning(
    model_id: &str,
    efforts: Option<&[String]>,
    default_effort: Option<&str>,
) -> Result<(), ProviderCatalogError> {
    if let Some(efforts) = efforts {
        let unique = efforts.iter().map(String::as_str).collect::<HashSet<_>>();
        if unique.len() != efforts.len()
            || efforts
                .iter()
                .any(|effort| !REASONING_EFFORTS.contains(&effort.as_str()))
        {
            return Err(ProviderCatalogError(format!(
                "语言模型 {model_id} 的思考档位无效"
            )));
        }
    }
    if let Some(default_effort) = default_effort {
        if !REASONING_EFFORTS.contains(&default_effort)
            || efforts.is_some_and(|efforts| {
                !efforts.is_empty() && !efforts.contains(&default_effort.to_string())
            })
        {
            return Err(ProviderCatalogError(format!(
                "语言模型 {model_id} 的默认思考档位无效"
            )));
        }
    } else if efforts.is_some_and(|efforts| !efforts.is_empty()) {
        return Err(ProviderCatalogError(format!(
            "语言模型 {model_id} 配置思考档位时必须指定默认值"
        )));
    }
    Ok(())
}

fn validate_protocols(
    provider_id: &str,
    protocols: &[ProviderProtocol],
) -> Result<(), ProviderCatalogError> {
    if protocols.is_empty() {
        return Err(ProviderCatalogError(format!(
            "供应商 {provider_id} 未配置协议"
        )));
    }
    let mut previous_position = None;
    for protocol in protocols {
        let position = ProviderProtocol::ORDERED
            .iter()
            .position(|candidate| candidate == protocol)
            .expect("all protocol enum variants are ordered");
        if previous_position.is_some_and(|previous| position <= previous) {
            return Err(ProviderCatalogError(format!(
                "供应商 {provider_id} 的 protocols 未按固定顺序排列"
            )));
        }
        previous_position = Some(position);
    }
    Ok(())
}

fn validate_protocol_base_urls(
    provider_id: &str,
    provider: &RawProvider,
    base_url: &str,
) -> Result<Vec<(ProviderProtocol, String)>, ProviderCatalogError> {
    let mut urls = Vec::new();
    for protocol in ProviderProtocol::ORDERED {
        let key = protocol_key(protocol);
        if let Some(url) = provider.protocol_base_urls.get(key) {
            if !provider.protocols.contains(&protocol) {
                return Err(ProviderCatalogError(format!(
                    "供应商 {provider_id} 为未声明协议 {key} 配置了 URL"
                )));
            }
            urls.push((protocol, required_value("协议 URL", url)?));
        }
    }
    if provider.protocol_base_urls.len() != urls.len() {
        return Err(ProviderCatalogError(format!(
            "供应商 {provider_id} 配置了未知协议 URL"
        )));
    }
    if base_url.is_empty() {
        return Err(ProviderCatalogError(format!(
            "供应商 {provider_id} 的默认 URL 不能为空"
        )));
    }
    Ok(urls)
}

fn validate_provider_model_references<T>(
    provider_id: &str,
    category: &str,
    model_ids: &[String],
    models: &BTreeMap<String, T>,
) -> Result<(), ProviderCatalogError> {
    let mut unique_models = HashSet::new();
    for model_id in model_ids {
        if !unique_models.insert(model_id.as_str()) {
            return Err(ProviderCatalogError(format!(
                "供应商 {provider_id} 重复引用{category} {model_id}"
            )));
        }
        if !models.contains_key(model_id) {
            return Err(ProviderCatalogError(format!(
                "供应商 {provider_id} 引用了未知{category} {model_id}"
            )));
        }
    }
    Ok(())
}

fn language_model_response(model: &CatalogLanguageModel) -> CatalogLanguageModelResponse {
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

fn image_generation_model_response(
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

fn provider_response(provider: &CatalogProvider) -> CatalogProviderResponse {
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

fn protocol_key(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::ChatCompletions => "chat_completions",
        ProviderProtocol::Responses => "responses",
        ProviderProtocol::AnthropicMessages => "anthropic_messages",
        ProviderProtocol::ImagesGenerations => "images_generations",
    }
}
