use std::time::Duration;

use prelay_protocol::{ProviderQuota, QuotaBalanceResponse};
use serde::Deserialize;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<ProviderQuota, String> {
    let url = balance_url(base_url);
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| "DeepSeek 余额接口请求失败".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "DeepSeek 余额接口返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|error| format!("DeepSeek 余额响应无法解析: {error}"))?;
    parse_balance(&body)
}

fn parse_balance(body: &str) -> Result<ProviderQuota, String> {
    let body = serde_json::from_str::<BalanceResponse>(body)
        .map_err(|error| format!("DeepSeek 余额响应无法解析: {error}"))?;
    let balance = body
        .balance_infos
        .iter()
        .find(|balance| balance.currency == "CNY")
        .or_else(|| {
            body.balance_infos
                .iter()
                .find(|balance| balance.currency == "USD")
        })
        .or_else(|| body.balance_infos.first())
        .ok_or_else(|| "DeepSeek 余额响应没有币种信息".to_string())?;
    Ok(ProviderQuota::Balance(QuotaBalanceResponse {
        available: body.is_available,
        amount: balance.total_balance.clone(),
        currency: balance.currency.clone(),
    }))
}

fn balance_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    if base.is_empty() {
        "https://api.deepseek.com/user/balance".to_string()
    } else {
        format!("{base}/user/balance")
    }
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
    use super::{balance_url, parse_balance};

    #[test]
    fn strips_the_v1_suffix() {
        assert_eq!(
            balance_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/user/balance"
        );
        assert_eq!(
            balance_url("https://api.deepseek.com"),
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
        assert_eq!(
            quota,
            prelay_protocol::ProviderQuota::Balance(prelay_protocol::QuotaBalanceResponse {
                available: true,
                amount: "110.00".to_string(),
                currency: "CNY".to_string(),
            })
        );
    }
}
