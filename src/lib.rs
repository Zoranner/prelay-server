pub mod bootstrap;
pub mod codec;
pub mod domain;
pub mod error;
pub mod inbound;
pub mod outbound;
pub mod service;
pub mod store;

use error::AppError;

#[derive(Clone)]
pub struct AppState {
    pub provider_catalog: std::sync::Arc<domain::catalog::ProviderCatalog>,
    pub storage: store::storage::Storage,
    pub activity_store: service::activity::ActivityStore,
    pub client: reqwest::Client,
    pub provider_clients: outbound::client::ProviderClientCache,
    pub client_update: outbound::integrations::client_update::ClientUpdateCache,
    pub extensions: outbound::integrations::extensions::ExtensionCatalog,
    pub failover: service::failover::FailoverRegistry,
}

impl AppState {
    /// 供应商的上游客户端：目录条目配了代理就用带代理的，没配就用直连客户端。
    pub fn provider_client(&self, provider_type: &str) -> Result<reqwest::Client, AppError> {
        let proxy_url = self.provider_catalog.provider_proxy_url(provider_type);
        let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(self.client.clone());
        };
        self.provider_clients
            .client(proxy_url, outbound::client::policy())
            .map_err(|error| AppError::Upstream {
                status: None,
                message: format!("供应商代理地址不可用 {proxy_url}: {error}"),
            })
    }
}

pub use bootstrap::test_support;
