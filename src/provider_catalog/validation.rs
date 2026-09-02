use std::collections::{BTreeMap, HashSet};

use super::{
    ProviderCatalogError, ProviderProtocol, RawImageGenerationModel, RawLanguageModel, RawProvider,
    INPUT_MODALITIES, OUTPUT_MODALITIES, REASONING_EFFORTS,
};

pub(super) fn validate_language_model(
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

pub(super) fn validate_image_generation_model(
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

pub(super) fn validate_protocols(
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

pub(super) fn validate_protocol_base_urls(
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

pub(super) fn validate_provider_model_references<T>(
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

fn required_value(label: &str, value: &str) -> Result<String, ProviderCatalogError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ProviderCatalogError(format!("{label}不能为空")));
    }
    Ok(value.to_string())
}

fn protocol_key(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::ChatCompletions => "chat_completions",
        ProviderProtocol::Responses => "responses",
        ProviderProtocol::AnthropicMessages => "anthropic_messages",
        ProviderProtocol::ImagesGenerations => "images_generations",
    }
}
