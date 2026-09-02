use std::{
    collections::{BTreeMap, HashSet},
    fmt, fs,
    path::Path,
};

use prelay_protocol::{ModelType, ProviderAuthScheme, ProviderProtocol};
use serde::Deserialize;

const REASONING_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    models: BTreeMap<String, CatalogModel>,
    providers: BTreeMap<String, CatalogProvider>,
}

#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub model_type: ModelType,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub auth_scheme: ProviderAuthScheme,
    pub base_url: String,
    pub protocols: Vec<ProviderProtocol>,
    pub protocol_base_urls: Vec<(ProviderProtocol, String)>,
    pub models: Vec<String>,
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
        let models = load_models(&directory.join("models.toml"))?;
        let providers = load_providers(&directory.join("providers.toml"), &models)?;
        Ok(Self { models, providers })
    }

    pub fn model(&self, model_id: &str) -> Option<&CatalogModel> {
        self.models.get(model_id)
    }

    pub fn provider(&self, provider_id: &str) -> Option<&CatalogProvider> {
        self.providers.get(provider_id)
    }
}

#[derive(Debug, Deserialize)]
struct ModelsDocument {
    models: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    id: String,
    display_name: String,
    model_type: ModelType,
    reasoning_efforts: Vec<String>,
    default_reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProvidersDocument {
    providers: Vec<RawProvider>,
}

#[derive(Debug, Deserialize)]
struct RawProvider {
    id: String,
    name: String,
    auth_scheme: ProviderAuthScheme,
    base_url: String,
    protocols: Vec<ProviderProtocol>,
    #[serde(default)]
    protocol_base_urls: BTreeMap<String, String>,
    models: Vec<String>,
}

fn load_models(path: &Path) -> Result<BTreeMap<String, CatalogModel>, ProviderCatalogError> {
    let contents = read_catalog_file(path)?;
    let document: ModelsDocument = toml::from_str(&contents)
        .map_err(|error| ProviderCatalogError(format!("无法解析 {}: {error}", path.display())))?;
    let mut models = BTreeMap::new();

    for model in document.models {
        let id = required_value("模型 ID", &model.id)?;
        let display_name = required_value("模型显示名称", &model.display_name)?;
        validate_model(&id, &model)?;
        if models.contains_key(&id) {
            return Err(ProviderCatalogError(format!("模型 ID 重复: {id}")));
        }
        models.insert(
            id.clone(),
            CatalogModel {
                id,
                display_name,
                model_type: model.model_type,
                reasoning_efforts: model.reasoning_efforts,
                default_reasoning_effort: model.default_reasoning_effort,
            },
        );
    }

    Ok(models)
}

fn load_providers(
    path: &Path,
    models: &BTreeMap<String, CatalogModel>,
) -> Result<BTreeMap<String, CatalogProvider>, ProviderCatalogError> {
    let contents = read_catalog_file(path)?;
    let document: ProvidersDocument = toml::from_str(&contents)
        .map_err(|error| ProviderCatalogError(format!("无法解析 {}: {error}", path.display())))?;
    let mut providers = BTreeMap::new();

    for provider in document.providers {
        let id = required_value("供应商 ID", &provider.id)?;
        let name = required_value("供应商名称", &provider.name)?;
        let base_url = required_value("供应商默认 URL", &provider.base_url)?;
        validate_protocols(&id, &provider.protocols)?;
        let protocol_base_urls = validate_protocol_base_urls(&id, &provider, &base_url)?;
        validate_provider_models(&id, &provider.models, models)?;
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
                models: provider.models,
            },
        );
    }

    Ok(providers)
}

fn read_catalog_file(path: &Path) -> Result<String, ProviderCatalogError> {
    fs::read_to_string(path)
        .map_err(|error| ProviderCatalogError(format!("无法读取 {}: {error}", path.display())))
}

fn required_value(label: &str, value: &str) -> Result<String, ProviderCatalogError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ProviderCatalogError(format!("{label}不能为空")));
    }
    Ok(value.to_string())
}

fn validate_model(model_id: &str, model: &RawModel) -> Result<(), ProviderCatalogError> {
    let efforts = model
        .reasoning_efforts
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if efforts.len() != model.reasoning_efforts.len()
        || model
            .reasoning_efforts
            .iter()
            .any(|effort| !REASONING_EFFORTS.contains(&effort.as_str()))
    {
        return Err(ProviderCatalogError(format!(
            "模型 {model_id} 的思考档位无效"
        )));
    }
    match model.model_type {
        ModelType::Image if !model.reasoning_efforts.is_empty() => {
            return Err(ProviderCatalogError(format!(
                "图像模型 {model_id} 不能配置思考档位"
            )));
        }
        ModelType::Image if model.default_reasoning_effort.is_some() => {
            return Err(ProviderCatalogError(format!(
                "图像模型 {model_id} 不能配置默认思考档位"
            )));
        }
        ModelType::Text => {
            if let Some(default_effort) = &model.default_reasoning_effort {
                if !efforts.contains(default_effort.as_str()) {
                    return Err(ProviderCatalogError(format!(
                        "模型 {model_id} 的默认思考档位不在支持列表中"
                    )));
                }
            } else if !model.reasoning_efforts.is_empty() {
                return Err(ProviderCatalogError(format!(
                    "模型 {model_id} 配置思考档位时必须指定默认值"
                )));
            }
        }
        ModelType::Image => {}
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

fn validate_provider_models(
    provider_id: &str,
    model_ids: &[String],
    models: &BTreeMap<String, CatalogModel>,
) -> Result<(), ProviderCatalogError> {
    let mut unique_models = HashSet::new();
    for model_id in model_ids {
        if !unique_models.insert(model_id.as_str()) {
            return Err(ProviderCatalogError(format!(
                "供应商 {provider_id} 重复引用模型 {model_id}"
            )));
        }
        if !models.contains_key(model_id) {
            return Err(ProviderCatalogError(format!(
                "供应商 {provider_id} 引用了未知模型 {model_id}"
            )));
        }
    }
    Ok(())
}

fn protocol_key(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::ChatCompletions => "chat_completions",
        ProviderProtocol::Responses => "responses",
        ProviderProtocol::AnthropicMessages => "anthropic_messages",
        ProviderProtocol::ImagesGenerations => "images_generations",
    }
}
