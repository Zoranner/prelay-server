use std::time::Duration;

use prelay_protocol::{QuotaError, QuotaErrorCode};

use crate::domain::catalog::CatalogQuota;

use super::{http_quota_error, quota_error, resolve_quota_url};

pub(crate) async fn fetch_quota_body(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
    vendor: &str,
) -> Result<String, QuotaError> {
    let url = resolve_quota_url(base_url, &quota.url);
    fetch_quota_url_body(client, &url, quota, api_key, vendor).await
}

pub(crate) async fn fetch_quota_url_body(
    client: &reqwest::Client,
    url: &str,
    quota: &CatalogQuota,
    api_key: &str,
    vendor: &str,
) -> Result<String, QuotaError> {
    let request = match quota.auth {
        crate::domain::catalog::QuotaAuth::Bearer => client.get(url).bearer_auth(api_key),
        crate::domain::catalog::QuotaAuth::Raw => client
            .get(url)
            .header(reqwest::header::AUTHORIZATION, api_key),
    };
    let response = request
        .timeout(Duration::from_secs(quota.timeout_secs))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            quota_error(
                QuotaErrorCode::Unreachable,
                format!("{vendor} 额度接口请求失败"),
            )
        })?;
    if !response.status().is_success() {
        return Err(http_quota_error(vendor, response.status()));
    }
    response.text().await.map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("{vendor} 额度响应无法解析: {error}"),
        )
    })
}
