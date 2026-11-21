use std::time::Duration;

use crate::domain::catalog::{CatalogQuota, QuotaAuth};
use prelay_protocol::{ProviderQuota, QuotaBalance, QuotaBalancePart, QuotaError, QuotaErrorCode};
use serde::Deserialize;

use super::{quota_error, resolve_quota_url};

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let url = resolve_quota_url(base_url, &quota.url);
    let request = match quota.auth {
        QuotaAuth::Bearer => client.get(&url).bearer_auth(api_key),
        QuotaAuth::Raw => client
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, api_key),
    };
    let response = request
        .timeout(Duration::from_secs(quota.timeout_secs))
        .send()
        .await
        .map_err(|_| quota_error(QuotaErrorCode::Unreachable, "DeepSeek 余额接口请求失败"))?;
    if !response.status().is_success() {
        return Err(http_error(response.status()));
    }
    let body = response.text().await.map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("DeepSeek 余额响应无法解析: {error}"),
        )
    })?;
    parse_balance(&body)
}

fn parse_balance(body: &str) -> Result<ProviderQuota, QuotaError> {
    let body = serde_json::from_str::<BalanceResponse>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("DeepSeek 余额响应无法解析: {error}"),
        )
    })?;
    if body.balance_infos.is_empty() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "DeepSeek 余额响应没有币种信息",
        ));
    }
    let mut infos = body.balance_infos;
    infos.sort_by_key(|info| match info.currency.as_str() {
        "CNY" => 0,
        "USD" => 1,
        _ => 2,
    });
    let parts = infos
        .iter()
        .map(|info| QuotaBalancePart {
            currency: info.currency.clone(),
            amount: info.total_balance.clone(),
        })
        .collect::<Vec<_>>();
    Ok(ProviderQuota {
        plan: None,
        windows: Vec::new(),
        balance: Some(QuotaBalance {
            parts,
            available: body.is_available,
        }),
        error: None,
        read_at: None,
        stale: false,
    })
}

fn http_error(status: reqwest::StatusCode) -> QuotaError {
    let code = match status.as_u16() {
        401 | 403 => QuotaErrorCode::Unauthorized,
        500..=599 => QuotaErrorCode::Unreachable,
        _ => QuotaErrorCode::InvalidResponse,
    };
    quota_error(
        code,
        format!("DeepSeek 余额接口返回 HTTP {}", status.as_u16()),
    )
}

#[derive(Deserialize)]
struct BalanceResponse {
    is_available: bool,
    #[serde(default)]
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Deserialize)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
}

#[cfg(test)]
mod tests {
    use super::parse_balance;
    use crate::service::quota::resolve_quota_url;

    #[test]
    fn resolves_a_relative_quota_url() {
        assert_eq!(
            resolve_quota_url("https://api.deepseek.com/v1", "/user/balance"),
            "https://api.deepseek.com/user/balance"
        );
        assert_eq!(
            resolve_quota_url(
                "https://api.deepseek.com/v1",
                "https://api.deepseek.com/user/balance"
            ),
            "https://api.deepseek.com/user/balance"
        );
    }

    #[test]
    fn parses_the_cny_balance() {
        let quota = parse_balance(
            r#"{
                "is_available": true,
                "balance_infos": [
                    { "currency": "USD", "total_balance": "1.00" },
                    { "currency": "CNY", "total_balance": "110.00" }
                ]
            }"#,
        )
        .expect("balance parses");
        let balance = quota.balance.expect("balance");
        assert!(quota.windows.is_empty());
        assert_eq!(quota.error, None);
        assert!(balance.available);
        assert_eq!(
            balance.parts,
            vec![
                prelay_protocol::QuotaBalancePart {
                    currency: "CNY".to_string(),
                    amount: "110.00".to_string(),
                },
                prelay_protocol::QuotaBalancePart {
                    currency: "USD".to_string(),
                    amount: "1.00".to_string(),
                },
            ]
        );
    }
}
